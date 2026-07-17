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
use base64::Engine as _;
use chrono::Utc;
use kaeru_rig::KaeruMemory;
use octo_code::code_tools;
use octo_core::{
    Blob, ChannelId, Cogitator, CogitatorContext, ConnectorId, Envelope, EventId, EventKind,
    Filter, OctoResult, ReplyChannel, Subscription,
};
use octo_rig::OctoDispatchTool;
use rig::{
    agent::{AgentBuilder, NoToolConfig},
    client::CompletionClient,
    completion::{CompletionModel, Message, Prompt},
    http_client::{HeaderMap, HeaderValue},
    message::{ImageMediaType, UserContent},
    providers::{openai, openrouter::Client as OpenRouterClient},
    OneOrMany,
};
use serde_json::{json, Value};
use tokio::{select, spawn};
use tracing::{debug, info, warn};

use crate::{
    acl::command as acl_command,
    codex_http::CodexHttp,
    codex_model::CodexResponsesModel,
    config::{AuthMode, Config},
    history::{to_messages, HistoryStore, Turn},
    openai_auth::{ensure_fresh, Subscription as SubscriptionAuth},
    prompt::PromptFiles,
    routines::seed_base_routine,
    scratchpad::ScratchpadStore,
    skills::SkillStore,
    status::StatusFeed,
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
    skills: Arc<SkillStore>,
    prompt: Arc<PromptFiles>,
}

impl AlbertCogitator {
    pub fn new(
        id: impl Into<String>,
        config: Config,
        history: Arc<dyn HistoryStore>,
        kaeru: KaeruMemory,
        scratchpad: Arc<ScratchpadStore>,
        skills: Arc<SkillStore>,
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
            skills,
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
                // A text payload is a normal turn; a Blob payload is an image the
                // connector downloaded (photo / image document), its caption in tags.
                let input = if let Some(text) = incoming.payload_as::<String>() {
                    Some(UserInput { text: text.clone(), image: None })
                } else {
                    incoming.payload_as::<Blob>().filter(|b| b.is_image()).map(|blob| UserInput {
                        text: incoming.tags.get("caption").cloned().unwrap_or_default(),
                        image: Some(blob.clone()),
                    })
                };
                if let Some(input) = input {
                    self.respond(incoming, input, ctx).await;
                }
            }
            "alarm.fired" => self.on_alarm(incoming, ctx).await,
            _ => {}
        }
    }

    /// A user message → a normal agent turn with memory + scheduler + scratchpad tools.
    async fn respond(self: &Arc<Self>, incoming: Arc<Envelope>, input: UserInput, ctx: &CogitatorContext) {
        let channel_key = channel_of(&incoming);

        // Reflexes fire on text-only turns: instant, no LLM.
        if input.image.is_none() {
            if let Some(canned) = command_reply(&input.text) {
                self.emit_reply(&incoming, canned.clone(), ctx).await;
                self.record(&channel_key, input.text, "(reflex reply)".into()).await;
                return;
            }

            // Reflex: owner-only ACL admin, deterministic (out of the LLM).
            if let Some(reply) = acl_command(&self.self_source, &input.text, &incoming, ctx).await {
                self.emit_reply(&incoming, reply, ctx).await;
                self.record(&channel_key, input.text, "(acl command)".into()).await;
                return;
            }
        }

        // An image on a text-only model: say so instead of silently ignoring it.
        if input.image.is_some() && !self.config.multimodal {
            let reply = "Я получил изображение, но текущая модель не видит картинки — \
                         опиши словами, что на ней. (Или включи мультимодальную модель: \
                         `multimodal = true` + vision-модель в albert.toml.)"
                .to_string();
            self.emit_reply(&incoming, reply.clone(), ctx).await;
            self.record(&channel_key, input.transcript(), reply).await;
            return;
        }

        let shown = input.transcript();
        info!(source = %incoming.source, channel = %channel_key, "← {shown}");

        // Live feedback while the turn runs: the "typing…" indicator plus the
        // tool-use / thoughts status feed (rendered by the connector).
        self.emit_typing(incoming.source.clone(), incoming.channel.clone(), ctx).await;
        let feed = self.feed(ctx, incoming.source.clone(), incoming.channel.clone());

        let active = self.active_reminders(ctx).await;
        let pad = self.scratchpad.render(&channel_key);
        let history = to_messages(&self.history.load(&channel_key).await);

        let base = self.prompt.base();
        let preamble = format!(
            "{base}\n\n{}\n\nCurrent time: {}\n\n{}\n\n{}\n\n{}",
            incoming_context(&incoming, &channel_key),
            now_rfc3339(&self.config.timezone),
            active,
            pad,
            self.skills.catalog(),
        );

        let answer = self
            .run_agent(ctx, &channel_key, &preamble, input.prompt(), history, feed)
            .await;

        info!("→ {answer}");
        self.emit_reply(&incoming, answer.clone(), ctx).await;
        self.record(&channel_key, shown, answer).await;
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
        let target = ConnectorId::new(reply_via);
        self.emit_typing(target.clone(), Some(ChannelId::new(channel.clone())), ctx).await;
        let feed = self.feed(ctx, target.clone(), Some(ChannelId::new(channel.clone())));
        let answer = self.run_agent(ctx, &channel, &preamble, prompt, history, feed).await;

        self.emit_text(
            target,
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
                    .run_agent(ctx, "system/reflection", &preamble, prompt, Vec::new(), StatusFeed::silent())
                    .await;
                info!(summary = %out, "memory-reflection routine done");
            }
            other => warn!(routine = other, "unknown routine; ignored"),
        }
    }

    /// Build the LLM client per the configured auth mode, then run one rig
    /// tool-loop. The two modes yield different concrete model types, so the
    /// build-tools-and-run tail lives in the generic [`Self::drive`].
    async fn run_agent(
        &self,
        ctx: &CogitatorContext,
        channel: &str,
        preamble: &str,
        prompt: Message,
        history: Vec<Message>,
        feed: StatusFeed,
    ) -> String {
        let dispatch = OctoDispatchTool::new(ctx.bus(), self.self_source.clone(), catalog(ctx));
        match self.config.auth {
            AuthMode::ApiKey => {
                let Some(key) = self.config.api_key.as_deref() else {
                    return "(config: api-key auth but no key loaded)".into();
                };
                let client = match OpenRouterClient::new(key) {
                    Ok(c) => c,
                    Err(e) => return format!("(llm client error: {e})"),
                };
                self.drive(client.agent(&self.config.model).preamble(preamble), dispatch, channel, prompt, history, feed)
                    .await
            }
            AuthMode::Subscription => {
                // Load (and, if it's expiring, refresh) the OAuth tokens, then build
                // the Codex client for this turn.
                let sub = match ensure_fresh(&self.config.subscription_auth_json).await {
                    Ok(s) => s,
                    Err(e) => return format!("(subscription auth: {e})"),
                };
                let client = match self.subscription_client(&sub) {
                    Ok(c) => c,
                    Err(e) => return format!("(subscription auth: {e})"),
                };
                let model = CodexResponsesModel::make(&client, self.config.model.as_str());
                self.drive(AgentBuilder::new(model).preamble(preamble), dispatch, channel, prompt, history, feed)
                    .await
            }
        }
    }

    /// A ChatGPT-subscription rig client: rig's OpenAI provider (Responses API by
    /// default) pointed at the Codex backend, with the OAuth access token as the
    /// bearer and the account id in the mandatory `ChatGPT-Account-ID` header.
    fn subscription_client(&self, sub: &SubscriptionAuth) -> Result<openai::Client<CodexHttp>, String> {
        if let Some(plan) = &sub.plan {
            info!(plan, "subscription auth loaded");
        }
        let mut headers = HeaderMap::new();
        headers.insert(
            "chatgpt-account-id",
            HeaderValue::from_str(&sub.account_id).map_err(|e| format!("bad account id: {e}"))?,
        );
        headers.insert("originator", HeaderValue::from_static("codex_cli_rs"));
        openai::Client::builder()
            .base_url(&self.config.subscription_base_url)
            .api_key(sub.access_token.as_str())
            .http_headers(headers)
            .http_client(CodexHttp::default())
            .build()
            .map_err(|e| format!("client build: {e}"))
    }

    /// Attach the toolset (kaeru memory + dispatch + scratchpad + skills) to a
    /// fresh agent builder and run one bounded tool-loop. Generic over the model
    /// so both auth modes share it; `install` takes the no-tools-yet builder, so
    /// it runs before the dispatch/scratchpad tools are chained on.
    async fn drive<M>(
        &self,
        base: AgentBuilder<M, (), NoToolConfig>,
        dispatch: OctoDispatchTool,
        channel: &str,
        prompt: Message,
        history: Vec<Message>,
        feed: StatusFeed,
    ) -> String
    where
        M: CompletionModel + 'static,
    {
        let m = &self.kaeru;
        let pad = self.scratchpad.handle(channel);
        debug!(
            channel,
            clouds = !self.config.clouds.is_empty(),
            max_turns = self.config.max_tool_turns,
            "building agent + running tool-loop"
        );
        // Variant (b): install the cloud tools only when a cloud is configured, so an
        // unconfigured Albert never shows the model the 7 dead share/pull tools. Both
        // methods return the same builder type, so the tool tail below is shared.
        let installed = if self.config.clouds.is_empty() {
            m.install(base)
        } else {
            m.install_with_cloud(base)
        };
        let with_tools = installed
            .tool(dispatch)
            .tool(pad.goal())
            .tool(pad.step())
            .tool(pad.mark())
            .tool(pad.note())
            .tool(pad.clear())
            .tool(self.skills.list_tool())
            .tool(self.skills.apply_tool())
            .tool(self.skills.file_tool());
        // octo-code file tools (read/write/edit/list/glob/grep), jailed to
        // $OCTO_CODE_WORKSPACE — Albert's hands on a scratch working directory.
        let agent = code_tools!(with_tools).build();
        match agent
            .prompt(prompt)
            .with_hook(feed)
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

    /// Nudge the source connector's "typing…" indicator for the turn (Telegram
    /// keeps it alive until the reply lands; other connectors ignore the kind).
    async fn emit_typing(&self, target: ConnectorId, channel: Option<ChannelId>, ctx: &CogitatorContext) {
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
    fn feed(&self, ctx: &CogitatorContext, target: ConnectorId, channel: Option<ChannelId>) -> StatusFeed {
        if !self.config.stream_status {
            return StatusFeed::silent();
        }
        StatusFeed::new(ctx.bus(), self.self_source.clone(), target, channel)
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

/// One user turn as perceived: text, optionally with an image the connector
/// downloaded (a Telegram photo / image document; the caption rides in `text`).
struct UserInput {
    text: String,
    image: Option<Blob>,
}

impl UserInput {
    /// The turn as the rig prompt message: plain text, or image + caption for a
    /// vision model (base64 travels fine through both the OpenRouter and the
    /// Codex Responses providers).
    fn prompt(&self) -> Message {
        let Some(blob) = &self.image else {
            return Message::user(self.text.clone());
        };
        let b64 = base64::engine::general_purpose::STANDARD.encode(blob.bytes());
        let caption = if self.text.trim().is_empty() {
            "Пользователь прислал это изображение без подписи — рассмотри его и \
             отреагируй по контексту разговора."
        } else {
            self.text.as_str()
        };
        Message::User {
            content: OneOrMany::many(vec![
                UserContent::image_base64(b64, Some(media_type(blob.content_type())), None),
                UserContent::text(caption),
            ])
            .expect("two content items"),
        }
    }

    /// A text stand-in for logs and the history transcript (raw bytes don't
    /// belong in either).
    fn transcript(&self) -> String {
        match &self.image {
            None => self.text.clone(),
            Some(b) if self.text.trim().is_empty() => {
                format!("(прислал изображение, {})", b.content_type())
            }
            Some(b) => format!("(прислал изображение, {}) {}", b.content_type(), self.text),
        }
    }
}

/// Map a MIME content type onto rig's media-type enum (unknowns land on JPEG —
/// Telegram photos are JPEG re-encodes anyway).
fn media_type(content_type: &str) -> ImageMediaType {
    match content_type {
        "image/png" => ImageMediaType::PNG,
        "image/gif" => ImageMediaType::GIF,
        "image/webp" => ImageMediaType::WEBP,
        "image/heic" => ImageMediaType::HEIC,
        "image/heif" => ImageMediaType::HEIF,
        _ => ImageMediaType::JPEG,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_input_stays_a_plain_user_message() {
        let input = UserInput { text: "привет".into(), image: None };
        assert!(matches!(input.prompt(), Message::User { content } if content.len() == 1));
        assert_eq!(input.transcript(), "привет");
    }

    #[test]
    fn image_input_becomes_image_plus_caption() {
        let blob = Blob::new(vec![0xFFu8, 0xD8, 0xFF], "image/jpeg").with_filename("photo.jpg");
        let input = UserInput { text: "что на фото?".into(), image: Some(blob) };
        let Message::User { content } = input.prompt() else {
            panic!("expected a user message");
        };
        let items: Vec<_> = content.into_iter().collect();
        assert_eq!(items.len(), 2);
        assert!(matches!(items[0], UserContent::Image(_)));
        assert!(matches!(&items[1], UserContent::Text(t) if t.text == "что на фото?"));
        assert!(input.transcript().contains("image/jpeg"));
    }

    #[test]
    fn captionless_image_gets_a_default_instruction() {
        let blob = Blob::new(vec![1u8, 2, 3], "image/png");
        let input = UserInput { text: "  ".into(), image: Some(blob) };
        let Message::User { content } = input.prompt() else {
            panic!("expected a user message");
        };
        let items: Vec<_> = content.into_iter().collect();
        assert!(matches!(&items[1], UserContent::Text(t) if t.text.contains("без подписи")));
    }
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
             • пришли фото (или картинку файлом) — посмотрю и отвечу по ней\n\
             • пока думаю — показываю «печатает…» и ход работы с инструментами\n\
             • владельцу: /allow <chat_id>, /deny <chat_id>, /allowed — доступ к боту\n\
             • /start, /help → мгновенно, без модели"
                .to_string(),
        ),
        _ => None,
    }
}
