use std::{sync::atomic::Ordering, sync::Arc};

use octo_core::{CogitatorContext, Envelope};
use tokio::{spawn, sync::Mutex};
use tracing::{info, warn};

use super::{
    super::channel_of, super::retention::temporary, super::AlbertCogitator, super::UserInput,
    compatible_actor, turn_key, ActiveTurn, ChannelState,
};
use crate::{
    acl::is_owner, cogitator::messaging::LogicalWork, cogitator::session_runtime::FrozenActions,
    cogitator::session_runtime::FrozenInput, history::journal_messages, history::tool_trace,
    history::Turn, messaging::contract::WorkRef, messaging::store::WorkTerminal,
    sessions::actor::ActorToolPolicy, sessions::SessionContext,
};

impl AlbertCogitator {
    pub(in crate::cogitator) async fn spawn_turn(
        self: Arc<Self>,
        incoming: Arc<Envelope>,
        input: UserInput,
        ctx: &CogitatorContext,
    ) {
        let channel = channel_of(&incoming);
        if let Some(context) = incoming.context.get::<SessionContext>() {
            if !context.authorized().await {
                self.emit_reply(
                    &incoming,
                    "Account access changed. Sign in again before continuing.".into(),
                    ctx,
                )
                .await;
                return;
            }
        }
        let state = self
            .turns
            .lock()
            .unwrap()
            .entry(turn_key(&incoming))
            .or_insert_with(|| Arc::new(Mutex::new(ChannelState::default())))
            .clone();
        let mut locked = state.lock().await;
        let incoming_policy = incoming
            .context
            .get::<SessionContext>()
            .map(SessionContext::tool_policy)
            .unwrap_or(ActorToolPolicy::ExistingHost);
        let guest = incoming_policy == ActorToolPolicy::NoTools;
        let blocked = locked.active.as_ref().is_some_and(|active| {
            if active.tool_policy == ActorToolPolicy::NoTools {
                return false;
            }
            let previous = active.continuation.as_ref().unwrap_or(&active.feedback);
            !is_owner(&incoming) && !compatible_actor(previous, &incoming)
        });
        if blocked || (guest && locked.message.is_some()) {
            self.emit_reply(
                &incoming,
                "Albert is working on another request. Please try again shortly.".into(),
                ctx,
            )
            .await;
            return;
        }
        // The original request receipt is checked before touching the running
        // task. A duplicate must not cancel it or schedule another model call.
        let boundary = if incoming.context.get::<SessionContext>().is_some() {
            match self.append_native(&incoming, &input).await {
                Ok(receipt) if receipt.duplicate => return,
                Ok(receipt) => Some(receipt.record_ids[0] - 1),
                Err(error) => {
                    warn!(%error, "native input was not accepted");
                    self.emit_reply(
                        &incoming,
                        "Message was not accepted; no work started.".into(),
                        ctx,
                    )
                    .await;
                    return;
                }
            }
        } else {
            None
        };
        let message_checkpoint = if let Some(message) = locked.message.take() {
            message.stop(&self, ctx, WorkTerminal::Interrupted).await
        } else {
            Vec::new()
        };
        let interrupted = locked.interrupt().await;
        let reuse_checkpoint = interrupted.as_ref().is_some_and(|previous| {
            compatible_actor(
                previous.continuation.as_ref().unwrap_or(&previous.feedback),
                &incoming,
            )
        });
        let tool_policy = incoming
            .context
            .get::<SessionContext>()
            .map(SessionContext::tool_policy)
            .unwrap_or(ActorToolPolicy::ExistingHost);
        let tool_policy = interrupted
            .as_ref()
            .filter(|_| reuse_checkpoint)
            .map(|previous| previous.tool_policy.intersect(tool_policy))
            .unwrap_or(tool_policy);
        let incoming = if let Some(context) = incoming.context.get::<SessionContext>() {
            let mut context = context.clone();
            context.actor_tools = Some(tool_policy);
            Arc::new((*incoming).clone().with_context(context))
        } else {
            incoming
        };
        let mut records = Vec::new();
        if let Some(previous) = &interrupted {
            self.end_feedback(&previous.feedback, ctx).await;
            self.cancel_scope(&previous.scope, ctx).await;
            records.extend(tool_trace(&previous.checkpoint));
        }
        let snapshot = match boundary {
            Some(boundary) => self.snapshot_before(&channel, boundary).await,
            None => self
                .visible_snapshot(&channel)
                .await
                .map(|(messages, revision)| (messages, revision, Vec::new())),
        };
        let (mut visible, compact_id, actions) = match snapshot {
            Ok(snapshot) => snapshot,
            Err(_) => {
                self.emit_reply(
                    &incoming,
                    "Could not load conversation history; no work started.".into(),
                    ctx,
                )
                .await;
                return;
            }
        };
        let incoming = if boundary.is_some() {
            let mut envelope = (*incoming).clone();
            envelope.context.insert(FrozenActions(actions));
            envelope.context.insert(FrozenInput {
                before_id: boundary.expect("native boundary"),
                checkpoint: interrupted
                    .as_ref()
                    .filter(|_| reuse_checkpoint)
                    .map(|previous| journal_messages(&previous.checkpoint))
                    .unwrap_or_default()
                    .into_iter()
                    .chain(message_checkpoint.clone())
                    .collect(),
            });
            Arc::new(envelope)
        } else {
            incoming
        };
        let work = interrupted
            .as_ref()
            .filter(|_| reuse_checkpoint)
            .and_then(|previous| previous.continuation.as_ref())
            .and_then(|original| original.context.get::<LogicalWork>())
            .cloned()
            .unwrap_or_else(|| LogicalWork(WorkRef::Turn(incoming.id.to_string())));
        let mut contextual = (*incoming).clone();
        contextual.context.insert(work);
        let incoming = Arc::new(contextual);
        let (mut messages, owner) = if let Some(previous) = interrupted {
            let messages = if !reuse_checkpoint {
                visible
            } else if previous.compact_id == compact_id {
                previous.messages
            } else {
                visible.extend(journal_messages(&previous.checkpoint));
                visible
            };
            (
                messages,
                (!reuse_checkpoint || previous.owner) && is_owner(&incoming),
            )
        } else {
            (visible, is_owner(&incoming))
        };
        messages.extend(message_checkpoint);
        if boundary.is_none() {
            records.push(Turn::user(input.transcript_with_source(&incoming)));
        }
        if let Err(error) = self
            .append_host(
                &channel,
                &incoming.id.to_string(),
                "interrupted-checkpoint",
                records,
            )
            .await
        {
            warn!(%error, %channel, "cannot persist incoming message; refusing to start tools");
            self.emit_reply(
                &incoming,
                "I couldn't save your message, so I haven't started the work. Please try again."
                    .into(),
                ctx,
            )
            .await;
            return;
        }
        if temporary(incoming.context.get::<SessionContext>()) {
            info!(source = %incoming.source, %channel, "temporary input accepted");
        } else {
            info!(source = %incoming.source, %channel, "← {}", input.transcript());
        }
        messages.push(input.prompt_with_source(&incoming));
        let scope = format!(
            "{}/{}-{}",
            self.id,
            incoming.id,
            self.turn_seq.fetch_add(1, Ordering::Relaxed)
        );
        let incoming = self.fresh_feedback(&incoming, &scope);
        let feed = self
            .scope_feed(
                &incoming,
                self.feed(ctx, incoming.source.clone(), incoming.channel.clone()),
            )
            .await;
        let incoming = Arc::new((*incoming).clone().with_context(feed.clone()));
        let me = self.clone();
        let owned_ctx = ctx.clone();
        let owned_state = state.clone();
        let owned_scope = scope.clone();
        let owned_feed = feed.clone();
        let mut history = messages.clone();
        let prompt = history.pop().expect("new input was appended");
        let voice = input.voice;
        let continuation = Some(incoming.clone());
        let feedback = incoming.clone();
        let task = spawn(async move {
            Box::pin(me.run_turn(
                incoming,
                voice,
                prompt,
                history,
                owner,
                &owned_ctx,
                &owned_scope,
                owned_feed,
                owned_state,
            ))
            .await;
        });
        // Insert before releasing the gate; a fast completion cannot clear its
        // slot before registration (the old spawn/insert race).
        locked.active = Some(ActiveTurn {
            tool_policy,
            feedback,
            continuation,
            compact_id,
            scope,
            task,
            feed,
            messages,
            owner,
        });
    }
}
