use std::{sync::Arc, time::Duration};

use octo_core::{ChannelId, CogitatorContext, ConnectorId, Envelope};
use octo_rig::carry_out_restart;
use rig::completion::Message;
use serde_json::Value;
use tokio::time::sleep;
use tracing::{info, warn};

use super::{
    action_context, now_rfc3339, with_action_log, AlbertCogitator, ROUTINE_MEMORY_REFLECTION,
};
use crate::{history::to_messages, status::StatusFeed};

impl AlbertCogitator {
    /// Perform a restart the model requested this turn. The `restart` tool only records
    /// the target; firing it mid-turn would wind connectors down before the reply is
    /// sent (the reply would be lost). We wait until the reply has been emitted, give it
    /// a short grace to flush to the channel, then publish the control signal.
    pub(super) async fn apply_restart(&self, target: String, ctx: &CogitatorContext) {
        info!(target = %target, "restart requested — flushing reply, then carrying it out");
        sleep(Duration::from_millis(1500)).await;
        if let Err(e) = carry_out_restart(&ctx.bus(), &self.self_source, &target).await {
            warn!(error = %e, target = %target, "failed to publish restart control signal");
        }
    }

    /// An alarm fired → a system routine (silent) or a user reminder (message).
    pub(super) async fn on_alarm(
        self: &Arc<Self>,
        incoming: Arc<Envelope>,
        ctx: &CogitatorContext,
    ) {
        let payload = incoming
            .payload_as::<Value>()
            .cloned()
            .unwrap_or(Value::Null);
        // System routine (self-care, e.g. memory reflection) — internal, no user message.
        if let Some(routine) = payload.get("routine").and_then(Value::as_str) {
            self.run_routine(routine, ctx).await;
            return;
        }
        let task = payload
            .get("task")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let channel = payload
            .get("channel")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let reply_via = payload
            .get("reply_via")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let alarm_id = incoming.tags.get("alarm_id").cloned().unwrap_or_default();

        let (Some(channel), Some(reply_via)) = (channel, reply_via) else {
            warn!(
                alarm_id,
                "alarm.fired without channel/reply_via; cannot remind"
            );
            return;
        };
        info!(alarm_id, %task, channel, "reminder due");

        let turns = self.history.load(&channel).await;
        let history = to_messages(&turns);
        let base = self.prompt.base();
        let preamble = format!(
            "{base}\n\nAn internal reminder alarm just fired (alarm_id={alarm_id}) for the \
             memory task \"{task}\". Recall its details from memory if useful, then write a short, \
             friendly reminder to the user and ask them to tell you when it's done (so it can stop \
             repeating). Do NOT schedule anything now.\n\nCurrent time: {}{}",
            now_rfc3339(&self.config.timezone),
            action_context(&turns),
        );
        let prompt = Message::user(format!(
            "Reminder due for \"{task}\". Write the reminder message to the user."
        ));
        let target = ConnectorId::new(reply_via);
        self.emit_typing(target.clone(), Some(ChannelId::new(channel.clone())), ctx)
            .await;
        let feed = self.feed(ctx, target.clone(), Some(ChannelId::new(channel.clone())));
        let (answer, _) = self
            .run_agent(
                ctx,
                &channel,
                &preamble,
                prompt,
                history,
                Some(target.clone()),
                false,
                feed.clone(),
                None,
            )
            .await;

        self.emit_text(
            target,
            Some(ChannelId::new(channel.clone())),
            answer.clone(),
            None,
            None,
            ctx,
        )
        .await;
        self.record(
            &channel,
            format!("(reminder fired: {task})"),
            with_action_log(&answer, &feed.drain_actions()),
        )
        .await;
    }

    /// A system routine fired — internal self-care, no user message.
    pub(super) async fn run_routine(self: &Arc<Self>, routine: &str, ctx: &CogitatorContext) {
        match routine {
            ROUTINE_MEMORY_REFLECTION => {
                info!("running memory-reflection routine");
                let base = self.prompt.base();
                let preamble = format!(
                    "{base}\n\nPERIODIC MEMORY REFLECTION — internal maintenance, NO user \
                     message. Call kaeru_reflect for the maintenance work-list, then act on it: link \
                     orphans, resolve open reviews, synthesise what has settled, prune noise. Keep it \
                     brief and work silently.\n\nCurrent time: {}",
                    now_rfc3339(&self.config.timezone),
                );
                let prompt = Message::user("Run your memory reflection pass now.");
                let (out, _) = self
                    .run_agent(
                        ctx,
                        "system/reflection",
                        &preamble,
                        prompt,
                        Vec::new(),
                        None,
                        false,
                        StatusFeed::silent(),
                        None,
                    )
                    .await;
                info!(summary = %out, "memory-reflection routine done");
            }
            other => warn!(routine = other, "unknown routine; ignored"),
        }
    }
}
