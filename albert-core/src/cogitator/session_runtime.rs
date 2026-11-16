mod admission;
mod journal;

use std::{collections::HashMap, sync::Arc, sync::Mutex, sync::Weak};

use octo_core::{CogitatorContext, Envelope};
use octo_history::SessionId;
use rig::completion::Message;
use tokio::{sync::Mutex as AsyncMutex, sync::OwnedMutexGuard, sync::OwnedRwLockReadGuard};
use tracing::warn;

use super::{
    channel_of, context::UserInput, messaging::LogicalWork, turns::turn_key, turns::ChannelState,
    AlbertCogitator,
};
use crate::{
    context::window_messages, context::DiagnosticScope, history::assistant_turn,
    history::sessions::Backend, history::to_messages, history::Turn, messaging::contract::WorkRef,
    sessions::transport::TurnFeedback, sessions::SessionContext, status::StatusFeed,
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
        if !session.authorized().await {
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
        let feed = feed
            .with_feedback(incoming.context.get::<TurnFeedback>().cloned())
            .with_reattach(self.config.stream_status)
            .with_presentation_route(incoming.source.clone(), incoming.channel.clone());
        let Some(session) = incoming.context.get::<SessionContext>() else {
            return feed;
        };
        let feed = feed.with_diagnostics(Some(DiagnosticScope {
            lane: "native",
            session_id: Some(session.session_id().as_str().into()),
            work_id: incoming
                .context
                .get::<LogicalWork>()
                .map(|work| match &work.0 {
                    WorkRef::Turn(id) | WorkRef::Child(id) => id.clone(),
                }),
            attempt_id: incoming
                .context
                .get::<TurnFeedback>()
                .map(|feedback| feedback.id().to_owned())
                .unwrap_or_else(|| incoming.id.to_string()),
            run_id: None,
        }));
        let feed = feed
            .with_session(Some(session.session_id().as_str()))
            .with_session_context(Some(session.clone()))
            .with_attempt(incoming.id.to_string());
        match &self.session_history {
            Some(history) => feed.with_backend(history.handle(session.session_id()).await),
            None => feed,
        }
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
        let window = self.context_window(key, Some(boundary)).await?;
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
