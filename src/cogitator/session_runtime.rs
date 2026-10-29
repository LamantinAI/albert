use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
};

use octo_connector_telegram::TurnFeedback;
use octo_core::{CogitatorContext, Envelope};
use octo_history::{AppendReceipt, AppendRequest, SessionId};
use rig::completion::Message;
use serde_json::to_vec;
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard, OwnedRwLockReadGuard};
use tracing::warn;

use super::{
    channel_of,
    context::UserInput,
    native_requests::{NativeDecision, NativeRequest},
    turns::{turn_key, ChannelState},
    AlbertCogitator,
};
use crate::{
    context::window_messages,
    history::{assistant_turn, sessions::Backend, to_messages, Turn},
    sessions::{acceptance::SourceReceipt, SessionContext},
    sessions::{
        error::{SessionError, SessionResult},
        ingress::for_message,
    },
    status::StatusFeed,
};

pub(super) type RequestGates = Mutex<HashMap<(String, String), Weak<AsyncMutex<()>>>>;

struct RequestPermit {
    request: Mutex<Option<Vec<OwnedMutexGuard<()>>>>,
    lifecycle: Mutex<Option<OwnedRwLockReadGuard<()>>>,
}

#[derive(Clone)]
pub(super) struct FrozenActions(pub Vec<Turn>);

#[derive(Clone)]
pub(super) struct FrozenInput {
    pub before_id: i64,
    pub checkpoint: Vec<Message>,
}

impl AlbertCogitator {
    /// T1's trusted close event calls this after the backend fence commits and
    /// releases its lifecycle writer. It never targets the physical selector.
    pub(crate) async fn cancel_session(&self, id: &SessionId, ctx: &CogitatorContext) -> bool {
        self.cancel_channel(&("session".into(), format!("session/{id}")), ctx)
            .await
    }
    pub(super) async fn delivery_lease(
        &self,
        incoming: &Envelope,
    ) -> Result<Option<OwnedRwLockReadGuard<Backend>>, ()> {
        let (Some(history), Some(session)) = (
            &self.session_history,
            incoming.context.get::<SessionContext>(),
        ) else {
            return Ok(None);
        };
        if !session.is_live() {
            return Err(());
        }
        let lease = history
            .handle(session.session_id())
            .await
            .read_owned()
            .await;
        if *lease == Backend::Closed {
            Err(())
        } else {
            Ok(Some(lease))
        }
    }
    pub(super) async fn scope_feed(&self, incoming: &Envelope, feed: StatusFeed) -> StatusFeed {
        let feed = feed.with_feedback(incoming.context.get::<TurnFeedback>().cloned());
        let Some(session) = incoming.context.get::<SessionContext>() else {
            return feed;
        };
        let feed = feed
            .with_session(Some(session.session_id().as_str()))
            .with_session_context(Some(session.clone()))
            .with_attempt(incoming.id.to_string());
        match &self.session_history {
            Some(history) => feed.with_backend(history.handle(session.session_id()).await),
            None => feed,
        }
    }
    pub(super) async fn admit_message(
        &self,
        incoming: Arc<Envelope>,
        ctx: &CogitatorContext,
    ) -> SessionResult<Arc<Envelope>> {
        let Some(service) = &self.session_service else {
            return Ok(incoming);
        };
        let stamped = incoming.context.get::<SessionContext>().cloned();
        if incoming.source.as_str() == "web"
            && stamped
                .as_ref()
                .is_none_or(|context| context.actor_tools.is_none())
        {
            return Err(SessionError::Denied);
        }
        if let Some(session) = &stamped {
            if session.reply_route().connector() != incoming.source.as_str()
                || incoming.channel.as_ref().map(|channel| channel.as_str())
                    != Some(session.reply_route().channel())
            {
                return Err(SessionError::Denied);
            }
            service.validate_context(session).await?;
        }
        let native = NativeRequest::from_envelope(&incoming)?;
        let keys = native.lookup_keys();
        let mut ordered = keys.clone();
        ordered.sort();
        ordered.dedup();
        let mut permits = Vec::with_capacity(ordered.len());
        for key in ordered {
            let gate = {
                let mut gates = self.request_gates.lock().unwrap();
                gates.retain(|_, gate| gate.strong_count() > 0);
                match gates.get(&key).and_then(Weak::upgrade) {
                    Some(gate) => gate,
                    None => {
                        let gate = Arc::new(AsyncMutex::new(()));
                        gates.insert(key, Arc::downgrade(&gate));
                        gate
                    }
                }
            };
            permits.push(gate.lock_owned().await);
        }
        let admission = for_message(&incoming, ctx)?;
        let (binding, lifecycle) = match &self.session_history {
            Some(history) => {
                let (bindings, lease) = history.find_requests_leased(&keys).await?;
                (native.reconcile(bindings)?, Some(lease))
            }
            None => (None, None),
        };
        let decision = NativeDecision {
            request: native,
            replay: binding.clone(),
        };
        let session = match (stamped, binding) {
            (Some(stamped), Some(binding))
                if stamped.session_id() != binding.journal.session_id() =>
            {
                return Err(SessionError::Denied)
            }
            (Some(stamped), _) => stamped,
            (None, Some(binding)) => {
                service
                    .context_for_message(admission, binding.journal.session_id())
                    .await?
            }
            (None, None) => service.admit(admission).await?,
        };
        let mut admitted = (*incoming).clone();
        admitted.context.insert(session);
        admitted.context.insert(decision);
        admitted.context.insert(RequestPermit {
            request: Mutex::new(Some(permits)),
            lifecycle: Mutex::new(lifecycle),
        });
        Ok(Arc::new(admitted))
    }

    pub(super) async fn append_native(
        &self,
        incoming: &Envelope,
        input: &UserInput,
    ) -> Result<AppendReceipt, String> {
        let decision = incoming.context.get::<NativeDecision>().cloned();
        let native = match &decision {
            Some(decision) => decision.request.clone(),
            None => NativeRequest::from_envelope(incoming).map_err(|error| error.to_string())?,
        };
        let result = if incoming
            .context
            .get::<SessionContext>()
            .is_some_and(|session| !session.is_live())
        {
            Err("Session ended.".into())
        } else if let Some(binding) = decision
            .as_ref()
            .filter(|decision| !decision.request.aliases.is_empty())
            .and_then(|decision| decision.replay.as_ref())
        {
            Ok(binding.receipt.clone())
        } else {
            let mut digest = Sha256::new();
            digest.update(input.text.as_bytes());
            for image in &input.images {
                digest.update((image.bytes().len() as u64).to_be_bytes());
                digest.update(image.bytes());
            }
            let request = native.append_request(
                format!("{:x}", digest.finalize()),
                vec![Turn::user(input.transcript_with_source(incoming))],
            );
            if native.aliases.is_empty() {
                self.history
                    .append_journal(&channel_of(incoming), &request)
                    .await
                    .map_err(|error| error.to_string())
            } else if let Some(history) = &self.session_history {
                history
                    .append_aliased(&channel_of(incoming), &request, &native.aliases)
                    .await
                    .map_err(|error| error.to_string())
            } else {
                Err("Native aliases require a session journal.".into())
            }
        };
        if let Some(receipt) = incoming.context.get::<SourceReceipt>() {
            receipt.complete(result.clone());
        }
        if let Some(permit) = incoming.context.get::<RequestPermit>() {
            permit.lifecycle.lock().unwrap().take();
            permit.request.lock().unwrap().take();
        }
        result
    }

    pub(super) async fn append_host(
        &self,
        key: &str,
        scope: &str,
        phase: &str,
        turns: Vec<Turn>,
    ) -> Result<(), String> {
        if turns.is_empty() {
            return Ok(());
        }
        if self.session_history.is_none() {
            return self
                .history
                .append(key, &turns)
                .await
                .map_err(|error| error.to_string());
        }
        let bytes = to_vec(&turns).map_err(|error| error.to_string())?;
        self.history
            .append_journal(
                key,
                &AppendRequest {
                    namespace: "albert/host".into(),
                    idempotency_key: format!("{scope}/{phase}"),
                    digest: format!("{:x}", Sha256::digest(bytes)),
                    turns,
                },
            )
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    pub(super) async fn strict_history(&self, key: &str) -> Result<Vec<Turn>, String> {
        match &self.session_history {
            Some(history) => history
                .strict_load(key)
                .await
                .map_err(|error| error.to_string()),
            None => Ok(self.history.load(key).await),
        }
    }

    pub(super) async fn snapshot_before(
        &self,
        key: &str,
        boundary: i64,
    ) -> Result<(Vec<Message>, Option<i64>, Vec<Turn>), String> {
        if !self.history.retains_context() {
            let turns = self.strict_history(key).await?;
            return Ok((to_messages(&turns), None, turns));
        }
        let mut window = self
            .history
            .context(key)
            .await
            .map_err(|error| error.to_string())?;
        window.messages.retain(|message| message.id <= boundary);
        let revision = window.compact.as_ref().map(|compact| compact.id);
        let actions = window
            .messages
            .iter()
            .map(|message| message.turn.clone())
            .collect();
        Ok((window_messages(&window), revision, actions))
    }

    pub(super) fn logical_conversation(channel: &str, target: Option<&str>) -> (String, String) {
        if channel.starts_with("session/") {
            ("session".into(), channel.into())
        } else {
            (target.unwrap_or_default().into(), channel.into())
        }
    }

    pub(super) async fn accept_control(
        &self,
        incoming: &Envelope,
        input: &UserInput,
    ) -> Result<bool, String> {
        if self.session_history.is_none() {
            return Ok(true);
        }
        let state = self
            .turns
            .lock()
            .unwrap()
            .entry(turn_key(incoming))
            .or_insert_with(|| Arc::new(AsyncMutex::new(ChannelState::default())))
            .clone();
        let _gate = state.lock().await;
        self.append_native(incoming, input)
            .await
            .map(|receipt| !receipt.duplicate)
    }

    pub(super) async fn finish_reflex(
        &self,
        incoming: &Envelope,
        user: String,
        answer: String,
        ctx: &CogitatorContext,
        control: bool,
    ) -> bool {
        let channel = channel_of(incoming);
        let result = if self.session_history.is_some() {
            let input = UserInput {
                text: user,
                images: Vec::new(),
                seed: None,
                voice: None,
            };
            match self.append_native(incoming, &input).await {
                Ok(_) => {
                    self.append_host(
                        &channel,
                        &incoming.id.to_string(),
                        "reflex-result",
                        vec![assistant_turn(answer.clone())],
                    )
                    .await
                }
                Err(error) => Err(error),
            }
        } else {
            self.history
                .append(
                    &channel,
                    &[Turn::user(user), assistant_turn(answer.clone())],
                )
                .await
                .map_err(|error| error.to_string())
        };
        if let Err(error) = result {
            warn!(%error, "reflex result was not saved");
            return false;
        }
        if control {
            self.emit_control_reply(incoming, answer, ctx).await;
        } else {
            self.emit_reply(incoming, answer, ctx).await;
        }
        true
    }
}
