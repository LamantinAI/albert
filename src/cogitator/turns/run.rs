use std::sync::Arc;

use octo_core::{CogitatorContext, Envelope};
use rig::completion::Message;
use tokio::sync::Mutex;
use tracing::{info, warn};

use super::super::{
    action_context, channel_of, incoming_context, now_rfc3339, with_action_log, AlbertCogitator,
};
use super::ChannelState;
use crate::{
    history::{assistant_turn, journal_messages, tool_trace},
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
        self.emit_typing(incoming.source.clone(), incoming.channel.clone(), ctx)
            .await;
        // Operation status is independent of optional reasoning/tool streaming.
        let progress = StatusFeed::new(
            ctx.bus(),
            self.self_source.clone(),
            incoming.source.clone(),
            incoming.channel.clone(),
        );
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
        let active = self.active_reminders(ctx).await;
        let pad = self.scratchpad.render(&channel);
        let stored = self.visible_turns(&channel).await.unwrap_or_default();
        let preamble = format!(
            "{}\n\n{}\n\nCurrent time: {}\n\n{}\n\n{}\n\n{}{}",
            self.prompt.base(),
            incoming_context(&incoming, &channel),
            now_rfc3339(&self.config.timezone),
            active,
            pad,
            self.skills.catalog(),
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
            self.run_agent(
                ctx,
                &channel,
                &preamble,
                prompt,
                history,
                Some(incoming.source.clone()),
                owner,
                feed.clone(),
                Some(scope),
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
        let checkpoint = feed.checkpoint();
        let mut records: Vec<_> = tool_trace(&checkpoint).into_iter().collect();
        records.push(assistant_turn(with_action_log(
            &answer,
            &feed.drain_actions(),
        )));
        if let Err(error) = self.history.append(&channel, &records).await {
            warn!(%error, "failed to save completed turn");
        }
        info!("→ {answer}");
        self.emit_reply(&incoming, answer, ctx).await;
        locked.active = None;
        drop(locked);
        if let Some(target) = restart {
            self.apply_restart(target, ctx).await;
        }
    }
}
