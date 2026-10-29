use std::sync::Arc;

use octo_approval::Ingress;
use octo_core::{CogitatorContext, Envelope};
use rig::completion::Message;
use tokio::sync::Mutex;
use tracing::{info, warn};

use super::super::retention::temporary;
use super::super::{
    action_context, channel_of, incoming_context, now_rfc3339, scratchpad_scope, with_action_log,
    AlbertCogitator,
};
use super::ChannelState;
use crate::{
    cogitator::session_runtime::FrozenActions,
    history::{assistant_turn, journal_messages, tool_trace},
    sessions::SessionContext,
    status::StatusFeed,
};

impl AlbertCogitator {
    pub(super) async fn run_turn(
        self: &Arc<Self>,
        incoming: Arc<Envelope>,
        voice: Option<String>,
        mut prompt: Message,
        mut history: Vec<Message>,
        owner: bool,
        ctx: &CogitatorContext,
        scope: &str,
        feed: StatusFeed,
        state: Arc<Mutex<ChannelState>>,
    ) {
        let channel = channel_of(&incoming);
        self.emit_typing_for(&incoming, ctx).await;
        // Operation status is independent of optional reasoning/tool streaming.
        let progress = self
            .scope_feed(
                &incoming,
                StatusFeed::new(
                    ctx.bus(),
                    self.self_source.clone(),
                    incoming.source.clone(),
                    incoming.channel.clone(),
                ),
            )
            .await;
        if self.session_history.is_none() {
            match self.compact_history(&channel, false, true, &progress).await {
                Ok(true) => match self.visible_snapshot(&channel).await {
                    Ok((mut messages, revision)) => {
                        messages.pop();
                        history = messages;
                        let mut locked = state.lock().await;
                        if let Some(active) = locked.active.as_mut().filter(|a| a.scope == scope) {
                            active.compact_id = revision;
                            active.messages = history.clone();
                            active.messages.push(prompt.clone());
                        }
                    }
                    Err(error) => {
                        self.finish_context_reply(&incoming, error, ctx, scope, &state)
                            .await;
                        return;
                    }
                },
                Ok(false) => {}
                Err(error) => {
                    self.finish_context_reply(&incoming, error, ctx, scope, &state)
                        .await;
                    return;
                }
            }
        }
        if let Err(error) = self
            .compact_frozen_input(&incoming, &mut history, &prompt, scope, &state, &progress)
            .await
        {
            self.finish_context_reply(&incoming, error, ctx, scope, &state)
                .await;
            return;
        }
        let session = incoming.context.get::<SessionContext>().cloned();
        let active = self.active_reminders(ctx, session.as_ref()).await;
        let pad = self.scratchpad.render_scoped(&scratchpad_scope(
            &Self::logical_conversation(&channel, Some(incoming.source.as_str())),
            owner,
        ));
        let stored = if let Some(actions) = incoming.context.get::<FrozenActions>() {
            actions.0.clone()
        } else {
            self.visible_turns(&channel).await.unwrap_or_default()
        };
        let restricted = incoming
            .context
            .get::<SessionContext>()
            .is_some_and(SessionContext::chat_only);
        let soul = if restricted {
            "You are Albert, a helpful assistant. Reply in the user's language. Use only this conversation and its scoped scratchpad; you have no global memory, filesystem or connector access.".into()
        } else {
            self.prompt.base()
        };
        let skills = if restricted {
            String::new()
        } else {
            self.skills.catalog()
        };
        let preamble = format!(
            "{}\n\n{}\n\nCurrent time: {}\n\n{}\n\n{}\n\n{}{}",
            soul,
            incoming_context(&incoming, &channel),
            now_rfc3339(&self.config.timezone),
            active,
            pad,
            skills,
            action_context(&stored)
        );
        let hearing_error = if let Some(path) = voice {
            match self.hear(&incoming, &path, ctx, scope, &feed).await {
                Ok(text) => {
                    history.push(prompt.clone());
                    history.extend(journal_messages(&feed.snapshot()));
                    prompt = Message::user(text);
                    None
                }
                Err(error) => Some(error),
            }
        } else {
            None
        };
        let (answer, restart) = if let Some(error) = hearing_error {
            (error, None)
        } else {
            self.run_agent_scoped(
                ctx,
                &channel,
                &preamble,
                prompt,
                history,
                Some(incoming.source.clone()),
                owner,
                feed.clone(),
                Some(scope),
                incoming.context.get::<Ingress>().cloned(),
                session,
            )
            .await
        };

        // Reply commit and accepting a newer input cannot interleave. If the new
        // input won, this task has been aborted while waiting for the gate.
        let mut locked = state.lock().await;
        if !locked
            .active
            .as_ref()
            .is_some_and(|active| active.scope == scope)
        {
            return;
        }
        // A failed End keeps the existing volatile evidence fenced in RAM.
        // Only the trusted successful-close event purges this slot and feed.
        if incoming
            .context
            .get::<SessionContext>()
            .is_some_and(|session| !session.is_live())
        {
            self.end_feedback(&incoming, ctx).await;
            return;
        }
        let checkpoint = feed.checkpoint();
        let mut records: Vec<_> = tool_trace(&checkpoint).into_iter().collect();
        records.push(assistant_turn(with_action_log(
            &answer,
            &feed.drain_actions(),
        )));
        if let Err(error) = self.append_host(&channel, scope, "final", records).await {
            warn!(%error, "failed to save completed turn");
            self.emit_reply(&incoming, "I couldn't record the completed turn. Check the recorded tool outcomes before retrying actions.".into(), ctx).await;
            locked.active = None;
            return;
        }
        if temporary(incoming.context.get::<SessionContext>()) {
            info!(%channel, "temporary response completed");
        } else {
            info!("→ {answer}");
        }
        self.emit_reply(&incoming, answer, ctx).await;
        locked.active = None;
        drop(locked);
        if let Some(target) = restart {
            self.apply_restart(target, ctx).await;
        }
    }
}
