use std::sync::{atomic::Ordering, Arc};

use octo_core::{CogitatorContext, Envelope};
use tokio::{spawn, sync::Mutex};
use tracing::{info, warn};

use super::super::{channel_of, AlbertCogitator, UserInput};
use super::{turn_key, ActiveTurn, ChannelState};
use crate::{
    acl::is_owner,
    history::{journal_messages, tool_trace, Turn},
};

impl AlbertCogitator {
    pub(in crate::cogitator) async fn spawn_turn(
        self: Arc<Self>,
        incoming: Arc<Envelope>,
        input: UserInput,
        ctx: &CogitatorContext,
    ) {
        let channel = channel_of(&incoming);
        let state = self
            .turns
            .lock()
            .unwrap()
            .entry(turn_key(&incoming))
            .or_insert_with(|| Arc::new(Mutex::new(ChannelState::default())))
            .clone();
        let mut locked = state.lock().await;
        let interrupted = locked.interrupt().await;
        let mut records = Vec::new();
        if let Some(previous) = &interrupted {
            self.cancel_scope(&previous.scope, ctx).await;
            records.extend(tool_trace(&previous.checkpoint));
        }
        let (mut visible, compact_id) = match self.visible_snapshot(&channel).await {
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
        let (mut messages, owner) = if let Some(previous) = interrupted {
            let messages = if previous.compact_id == compact_id {
                previous.messages
            } else {
                visible.extend(journal_messages(&previous.checkpoint));
                visible
            };
            (messages, previous.owner && is_owner(&incoming))
        } else {
            (visible, is_owner(&incoming))
        };
        records.push(Turn::user(input.transcript_with_source(&incoming)));
        if let Err(error) = self.history.append(&channel, &records).await {
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
        info!(source = %incoming.source, %channel, "← {}", input.transcript());
        messages.push(input.prompt_with_source(&incoming));
        let scope = format!(
            "{}/{}-{}",
            self.id,
            incoming.id,
            self.turn_seq.fetch_add(1, Ordering::Relaxed)
        );
        let feed = self.feed(ctx, incoming.source.clone(), incoming.channel.clone());
        let me = self.clone();
        let owned_ctx = ctx.clone();
        let owned_state = state.clone();
        let owned_scope = scope.clone();
        let owned_feed = feed.clone();
        let mut history = messages.clone();
        let prompt = history.pop().expect("new input was appended");
        let voice = input.voice;
        let continuation = Some(incoming.clone());
        let task = spawn(async move {
            me.run_turn(
                incoming,
                voice,
                prompt,
                history,
                owner,
                &owned_ctx,
                &owned_scope,
                owned_feed,
                owned_state,
            )
            .await;
        });
        // Insert before releasing the gate; a fast completion cannot clear its
        // slot before registration (the old spawn/insert race).
        locked.active = Some(ActiveTurn {
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
