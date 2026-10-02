use std::time::Duration;

use octo_core::{
    ChannelId, CogitatorContext, ConnectorId, Envelope, EventId, EventKind, ReplyChannel,
};
use serde_json::{json, Value};
use tracing::warn;

use super::{AlbertCogitator, SCHEDULER_ID};
use crate::{
    history::{assistant_turn, Turn},
    status::StatusFeed,
};

impl AlbertCogitator {
    /// Query the scheduler for active alarms and render them for the preamble, so
    /// the model can cancel the right reminder by matching its task.
    pub(super) async fn active_reminders(&self, ctx: &CogitatorContext) -> String {
        let req = Envelope::new(
            self.self_source.clone(),
            EventKind::from_static("octo.scheduler.list_alarms"),
            json!({}),
        )
        .with_target(ConnectorId::new(SCHEDULER_ID));
        let resp = ctx
            .publish_and_await_response(req, Duration::from_secs(5))
            .await;
        let alarms = match resp {
            Ok(env) => env
                .payload_as::<Value>()
                .and_then(|v| v.get("alarms").cloned())
                .unwrap_or(Value::Array(vec![])),
            Err(_) => return "Active reminders: (none)".into(),
        };
        let items = alarms.as_array().cloned().unwrap_or_default();
        if items.is_empty() {
            return "Active reminders: (none)".into();
        }
        let lines: Vec<String> = items
            .iter()
            .map(|a| {
                let id = a.get("id").and_then(Value::as_str).unwrap_or("?");
                let task = a
                    .get("payload")
                    .and_then(|p| p.get("task"))
                    .and_then(Value::as_str)
                    .unwrap_or("?");
                let next = a.get("next_fire").and_then(Value::as_str).unwrap_or("?");
                format!("- alarm_id={id} task=\"{task}\" next_fire={next}")
            })
            .collect();
        format!(
            "Active reminders (cancel by alarm_id when the user finishes a task):\n{}",
            lines.join("\n")
        )
    }

    pub(super) async fn record(&self, channel: &str, user: String, assistant: String) {
        if let Err(e) = self
            .history
            .append(channel, &[Turn::user(user), assistant_turn(assistant)])
            .await
        {
            warn!(error = %e, "failed to persist history");
        }
    }

    /// Nudge the source connector's "typing…" indicator for the turn (Telegram
    /// keeps it alive until the reply lands; other connectors ignore the kind).
    pub(super) async fn emit_typing(
        &self,
        target: ConnectorId,
        channel: Option<ChannelId>,
        ctx: &CogitatorContext,
    ) {
        let mut env = Envelope::new(
            self.self_source.clone(),
            EventKind::from_static("chat.typing"),
            json!({}),
        )
        .with_target(target);
        if let Some(ch) = channel {
            env = env.with_channel(ch);
        }
        if let Err(e) = ctx.publish(env).await {
            warn!(error = %e, "failed to publish chat.typing");
        }
    }

    /// The turn's live status feed (tool calls / thoughts → `chat.status`), or a
    /// silent one when streaming is switched off in config.
    pub(super) fn feed(
        &self,
        ctx: &CogitatorContext,
        target: ConnectorId,
        channel: Option<ChannelId>,
    ) -> StatusFeed {
        if !self.config.stream_status {
            return StatusFeed::silent();
        }
        StatusFeed::new(ctx.bus(), self.self_source.clone(), target, channel)
    }

    pub(super) async fn emit_control_reply(
        &self,
        incoming: &Envelope,
        text: String,
        ctx: &CogitatorContext,
    ) {
        let mut reply = Envelope::new(
            self.self_source.clone(),
            EventKind::from_static("chat.reply"),
            text,
        )
        .with_target(incoming.source.clone())
        .with_correlation(incoming.id)
        .with_tag("control_reply", "true");
        if let Some(channel) = incoming.channel.clone() {
            reply = reply.with_channel(channel);
        }
        if let Some(metadata) = incoming.channel_metadata.clone() {
            reply = reply.with_channel_metadata(metadata);
        }
        if let Err(error) = ctx.publish(reply).await {
            warn!(%error,"could not send control reply");
        }
    }

    /// Reply to an incoming chat message: back to its source on the same channel.
    pub(super) async fn emit_reply(
        &self,
        incoming: &Envelope,
        text: String,
        ctx: &CogitatorContext,
    ) {
        self.emit_text(
            incoming.source.clone(),
            incoming.channel.clone(),
            text,
            Some(incoming.id),
            incoming.reply_to.clone(),
            ctx,
        )
        .await;
    }

    /// Emit a `chat.reply` with explicit target/channel (used by the reply and the
    /// reminder paths).
    pub(super) async fn emit_text(
        &self,
        target: ConnectorId,
        channel: Option<ChannelId>,
        text: String,
        correlation: Option<EventId>,
        reply_to: Option<ReplyChannel>,
        ctx: &CogitatorContext,
    ) {
        let mut reply = Envelope::new(
            self.self_source.clone(),
            EventKind::from_static("chat.reply"),
            text,
        )
        .with_target(target);
        if let Some(channel) = channel {
            reply = reply.with_channel(channel);
        }
        if let Some(cid) = correlation {
            reply = reply.with_correlation(cid);
        }
        if let Some(rt) = reply_to {
            reply = reply.with_reply_to(rt);
        }
        if let Err(e) = ctx.publish(reply).await {
            warn!(error = %e, "failed to publish chat.reply");
        }
    }
}
