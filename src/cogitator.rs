//! `AlbertCogitator` — an Octo cogitator with **kaeru memory** + the **scheduler**
//! connector, wired into a reminder loop.
//!
//! It's the octolab `ReactCogitator` grown up: still a rig native tool-loop (the
//! graph is Phase 2), but now it perceives two event kinds and carries persistent
//! memory:
//!
//! - `chat.message` → a normal turn. Tools: `dispatch_to_connector` (reaches the
//!   scheduler) + the kaeru memory verbs + the scratchpad.
//! - `alarm.fired`  → a user reminder is due (recall + remind) OR a system routine
//!   fired (silent self-care, e.g. memory reflection — see [`crate::routines`]).
//!
//! Owner-only ACL admin (`/allow` etc.) lives in [`crate::acl`]; the base routines
//! in [`crate::routines`].

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use chrono::Utc;
use kaeru_rig::KaeruMemory;
use octo_core::{
    ChannelId, Cogitator, CogitatorContext, ConnectorId, Envelope, EventId, EventKind, Filter,
    OctoResult, ReplyChannel, Subscription,
};
use octo_rig::OctoDispatchTool;
use rig::{
    client::CompletionClient,
    completion::{Message, Prompt},
    providers::openrouter::Client,
};
use serde_json::{json, Value};
use tokio::{select, spawn};
use tracing::{info, warn};

use crate::{
    acl::command as acl_command,
    config::Config,
    history::{to_messages, HistoryStore, Turn},
    prompt::PromptFiles,
    routines::seed_base_routine,
    scratchpad::ScratchpadStore,
};

pub(crate) const SCHEDULER_ID: &str = "scheduler";
/// Payload marker distinguishing a system routine alarm from a user reminder.
pub(crate) const ROUTINE_MEMORY_REFLECTION: &str = "memory_reflection";

pub struct AlbertCogitator {
    id: String,
    self_source: ConnectorId,
    config: Config,
    history: Arc<dyn HistoryStore>,
    kaeru: KaeruMemory,
    scratchpad: Arc<ScratchpadStore>,
    prompt: Arc<PromptFiles>,
}

impl AlbertCogitator {
    pub fn new(
        id: impl Into<String>,
        config: Config,
        history: Arc<dyn HistoryStore>,
        kaeru: KaeruMemory,
        scratchpad: Arc<ScratchpadStore>,
        prompt: Arc<PromptFiles>,
    ) -> Arc<Self> {
        let id = id.into();
        Arc::new(Self {
            self_source: ConnectorId::new(format!("cogitator/{id}")),
            id,
            config,
            history,
            kaeru,
            scratchpad,
            prompt,
        })
    }
}

#[async_trait]
impl Cogitator for AlbertCogitator {
    fn id(&self) -> &str {
        &self.id
    }

    fn filter(&self) -> Filter {
        // Perceive user messages and scheduler fires.
        Filter::by_kind("chat.message").with_kind("alarm.fired")
    }

    async fn run(
        self: Arc<Self>,
        ctx: CogitatorContext,
        mut subscription: Subscription,
    ) -> OctoResult<()> {
        // Seed Albert's base routines (memory reflection) once the scheduler is up.
        spawn(seed_base_routine(
            ctx.bus(),
            self.self_source.clone(),
            self.config.reflection_secs,
        ));
        loop {
            select! {
                next = subscription.next() => match next {
                    Some(envelope) => self.clone().handle(envelope, &ctx).await,
                    None => return Ok(()),
                },
                _ = ctx.shutdown.cancelled() => return Ok(()),
            }
        }
    }
}

impl AlbertCogitator {
    async fn handle(self: Arc<Self>, incoming: Arc<Envelope>, ctx: &CogitatorContext) {
        if incoming.source == self.self_source {
            return; // never react to our own emissions
        }
        match incoming.kind.as_str() {
            "chat.message" => {
                if let Some(text) = incoming.payload_as::<String>().cloned() {
                    self.respond(incoming, text, ctx).await;
                }
            }
            "alarm.fired" => self.on_alarm(incoming, ctx).await,
            _ => {}
        }
    }

    /// A user message → a normal agent turn with memory + scheduler + scratchpad tools.
    async fn respond(self: &Arc<Self>, incoming: Arc<Envelope>, text: String, ctx: &CogitatorContext) {
        let channel_key = channel_of(&incoming);

        // Reflex: instant, no LLM.
        if let Some(canned) = command_reply(&text) {
            self.emit_reply(&incoming, canned.clone(), ctx).await;
            self.record(&channel_key, text, "(reflex reply)".into()).await;
            return;
        }

        // Reflex: owner-only ACL admin, deterministic (out of the LLM).
        if let Some(reply) = acl_command(&self.self_source, &text, &incoming, ctx).await {
            self.emit_reply(&incoming, reply, ctx).await;
            self.record(&channel_key, text, "(acl command)".into()).await;
            return;
        }

        info!(source = %incoming.source, channel = %channel_key, "← {text}");

        let active = self.active_reminders(ctx).await;
        let pad = self.scratchpad.render(&channel_key);
        let history = to_messages(&self.history.load(&channel_key).await);

        let base = self.prompt.base();
        let preamble = format!(
            "{base}\n\n{}\n\nCurrent time: {}\n\n{}\n\n{}",
            incoming_context(&incoming, &channel_key),
            now_rfc3339(&self.config.timezone),
            active,
            pad,
        );

        let answer = self
            .run_agent(ctx, &channel_key, &preamble, Message::user(text.clone()), history)
            .await;

        info!("→ {answer}");
        self.emit_reply(&incoming, answer.clone(), ctx).await;
        self.record(&channel_key, text, answer).await;
    }

    /// An alarm fired → a system routine (silent) or a user reminder (message).
    async fn on_alarm(self: &Arc<Self>, incoming: Arc<Envelope>, ctx: &CogitatorContext) {
        let payload = incoming.payload_as::<Value>().cloned().unwrap_or(Value::Null);
        // System routine (self-care, e.g. memory reflection) — internal, no user message.
        if let Some(routine) = payload.get("routine").and_then(Value::as_str) {
            self.run_routine(routine, ctx).await;
            return;
        }
        let task = payload.get("task").and_then(Value::as_str).unwrap_or("").to_string();
        let channel = payload.get("channel").and_then(Value::as_str).map(str::to_owned);
        let reply_via = payload
            .get("reply_via")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let alarm_id = incoming.tags.get("alarm_id").cloned().unwrap_or_default();

        let (Some(channel), Some(reply_via)) = (channel, reply_via) else {
            warn!(alarm_id, "alarm.fired without channel/reply_via; cannot remind");
            return;
        };
        info!(alarm_id, %task, channel, "reminder due");

        let history = to_messages(&self.history.load(&channel).await);
        let base = self.prompt.base();
        let preamble = format!(
            "{base}\n\nAn internal reminder alarm just fired (alarm_id={alarm_id}) for the \
             memory task \"{task}\". Recall its details from memory if useful, then write a short, \
             friendly reminder to the user and ask them to tell you when it's done (so it can stop \
             repeating). Do NOT schedule anything now.\n\nCurrent time: {}",
            now_rfc3339(&self.config.timezone),
        );
        let prompt = Message::user(format!(
            "Reminder due for \"{task}\". Write the reminder message to the user."
        ));
        let answer = self.run_agent(ctx, &channel, &preamble, prompt, history).await;

        self.emit_text(
            ConnectorId::new(reply_via),
            Some(ChannelId::new(channel.clone())),
            answer.clone(),
            None,
            None,
            ctx,
        )
        .await;
        self.record(&channel, format!("(reminder fired: {task})"), answer)
            .await;
    }

    /// A system routine fired — internal self-care, no user message.
    async fn run_routine(&self, routine: &str, ctx: &CogitatorContext) {
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
                let out = self
                    .run_agent(ctx, "system/reflection", &preamble, prompt, Vec::new())
                    .await;
                info!(summary = %out, "memory-reflection routine done");
            }
            other => warn!(routine = other, "unknown routine; ignored"),
        }
    }

    /// Build the agent (dispatch + kaeru + scratchpad tools) and run one rig tool-loop.
    async fn run_agent(
        &self,
        ctx: &CogitatorContext,
        channel: &str,
        preamble: &str,
        prompt: Message,
        history: Vec<Message>,
    ) -> String {
        let client = match Client::new(&self.config.api_key) {
            Ok(c) => c,
            Err(e) => return format!("(llm client error: {e})"),
        };
        let dispatch = OctoDispatchTool::new(ctx.bus(), self.self_source.clone(), catalog(ctx));
        let m = &self.kaeru;
        let pad = self.scratchpad.handle(channel);
        let agent = client
            .agent(&self.config.model)
            .preamble(preamble)
            .tool(dispatch)
            .tool(m.awake())
            .tool(m.recall())
            .tool(m.read())
            .tool(m.remember())
            .tool(m.task())
            .tool(m.done())
            .tool(m.recent())
            .tool(m.reflect())
            .tool(m.link())
            .tool(m.synthesise())
            .tool(pad.goal())
            .tool(pad.step())
            .tool(pad.mark())
            .tool(pad.note())
            .tool(pad.clear())
            .build();
        match agent
            .prompt(prompt)
            .max_turns(self.config.max_tool_turns)
            .with_history(history)
            .await
        {
            Ok(a) => a,
            Err(e) => {
                warn!(error = %e, "llm tool-call failed");
                format!("(llm error: {e})")
            }
        }
    }

    /// Query the scheduler for active alarms and render them for the preamble, so
    /// the model can cancel the right reminder by matching its task.
    async fn active_reminders(&self, ctx: &CogitatorContext) -> String {
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

    async fn record(&self, channel: &str, user: String, assistant: String) {
        if let Err(e) = self
            .history
            .append(channel, &[Turn::user(user), Turn::assistant(assistant)])
            .await
        {
            warn!(error = %e, "failed to persist history");
        }
    }

    /// Reply to an incoming chat message: back to its source on the same channel.
    async fn emit_reply(&self, incoming: &Envelope, text: String, ctx: &CogitatorContext) {
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
    async fn emit_text(
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

fn channel_of(env: &Envelope) -> String {
    env.channel
        .as_ref()
        .map(|c| c.as_str().to_string())
        .unwrap_or_default()
}

/// Current time as RFC3339 in the owner's configured timezone (offset form,
/// e.g. `…+03:00`), so the agent's notion of "now" — and thus "today" and any
/// reminder times it computes — is local rather than UTC.
fn now_rfc3339(tz: &chrono_tz::Tz) -> String {
    Utc::now().with_timezone(tz).to_rfc3339()
}

/// Front-load the incoming envelope's provenance for the model — where the message
/// came from, so it can reply through the same channel and store it on reminders.
fn incoming_context(env: &Envelope, channel: &str) -> String {
    format!(
        "Context — this message arrived via connector \"{}\", channel \"{}\". Reply through this \
         same connector/channel; when scheduling a reminder, put channel=\"{channel}\" and \
         reply_via=\"{}\" into the alarm payload.",
        env.source, channel, env.source
    )
}

/// Catalogue of connectors advertising a description (env-as-tools) for the
/// dispatch tool.
fn catalog(ctx: &CogitatorContext) -> String {
    ctx.connectors()
        .iter()
        .filter_map(|c| {
            c.capabilities
                .description
                .as_ref()
                .map(|d| format!("- target \"{}\":\n{}", c.id, d))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn command_reply(text: &str) -> Option<String> {
    match text.trim() {
        "/start" => Some(
            "Привет! Я Альберт — ассистент на рантайме Octo с памятью (kaeru) и планировщиком. \
             Скажи «напомни …» — заведу напоминалку и буду напоминать, пока не скажешь, что сделал. \
             /help — подробнее."
                .to_string(),
        ),
        "/help" => Some(
            "Я помню контекст и умею напоминания:\n\
             • «напомни каждые 30 минут попить воды» → поставлю повторяющееся напоминание\n\
             • когда сработает — напишу; скажи «сделал» → отмечу выполненным и остановлю\n\
             • владельцу: /allow <chat_id>, /deny <chat_id>, /allowed — доступ к боту\n\
             • /start, /help → мгновенно, без модели"
                .to_string(),
        ),
        _ => None,
    }
}
