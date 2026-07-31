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

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use base64::Engine as _;
use chrono::Utc;
use kaeru_rig::KaeruMemory;
use octo_code::code_tools;
use octo_core::{
    Blob, ChannelId, Cogitator, CogitatorContext, ConnectorId, Envelope, EventId, EventKind,
    Filter, OctoResult, ReplyChannel, Subscription,
};
use octo_rig::{carry_out_restart, OctoDispatchTool, RestartTool, SendFileTool};
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
    acl::{command as acl_command, is_owner},
    codex_http::CodexHttp,
    codex_model::CodexResponsesModel,
    config::{AuthMode, Config},
    history::{recent_actions, to_messages, HistoryStore, Turn, ACTION_MARKER},
    selfconfig::SelfConfig,
    transcribe::{transcribe, MAX_INLINE_SECS},
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
                // A text payload is a normal turn; a Blob payload is media the
                // connector downloaded, its caption in tags — an image goes to the
                // model as-is, a voice message becomes text first (no Codex model
                // takes audio) and from there is an ordinary turn.
                let input = if let Some(text) = incoming.payload_as::<String>() {
                    Some(UserInput { text: text.clone(), image: None })
                } else if let Some(blob) = incoming.payload_as::<Blob>().filter(|b| b.is_image()) {
                    Some(UserInput {
                        text: incoming.tags.get("caption").cloned().unwrap_or_default(),
                        image: Some(blob.clone()),
                    })
                } else if let Some(blob) = incoming.payload_as::<Blob>().filter(|b| b.is_audio()) {
                    // `None` means we already told the user why we couldn't listen.
                    self.hear(&incoming, blob, ctx).await.map(|text| UserInput { text, image: None })
                } else {
                    None
                };
                if let Some(input) = input {
                    self.respond(incoming, input, ctx).await;
                }
            }
            "alarm.fired" => self.on_alarm(incoming, ctx).await,
            _ => {}
        }
    }

    /// Perceive a voice message: transcribe it and hand back the text so the turn
    /// proceeds as if it had been typed. `None` means the voice went unheard *and the
    /// user has been told why* — silence is the one unacceptable answer to someone who
    /// just spoke.
    async fn hear(
        self: &Arc<Self>,
        incoming: &Arc<Envelope>,
        blob: &Blob,
        ctx: &CogitatorContext,
    ) -> Option<String> {
        let decline = |reason: String| async move {
            self.emit_reply(incoming, reason.clone(), ctx).await;
            self.record(&channel_of(incoming), "(голосовое сообщение)".into(), reason).await;
            None::<String>
        };

        if !self.config.hearing {
            return decline(
                "Я получил голосовое, но сейчас не могу его расслышать: расшифровка идёт \
                 через подписку ChatGPT. (Включи `auth = \"subscription\"`, либо `hearing = \
                 true` в albert.toml, если знаешь, что делаешь.)"
                    .to_string(),
            )
            .await;
        }

        // The connector tags the length, so an over-long note is refused before a byte
        // is uploaded — the endpoint would otherwise truncate it in silence.
        let secs = incoming.tags.get("duration_secs").and_then(|s| s.parse::<u32>().ok());
        if secs.is_some_and(|s| s > MAX_INLINE_SECS) {
            return decline(format!(
                "Это голосовое на {} мин — длиннее {} мин я расшифровываю не в диалоге, иначе \
                 текст молча обрежется. Пришли файлом, и я разберу его целиком.",
                secs.unwrap_or_default() / 60,
                MAX_INLINE_SECS / 60,
            ))
            .await;
        }

        let sub = match ensure_fresh(&self.config.subscription_auth_json).await {
            Ok(sub) => sub,
            Err(e) => {
                warn!(error = %e, "voice: subscription token unavailable");
                return decline(
                    "Не смог расшифровать голосовое: подписка ChatGPT сейчас недоступна \
                     (токен просрочен — нужен `albert login`)."
                        .to_string(),
                )
                .await;
            }
        };

        let filename = blob.filename().unwrap_or("voice.ogg");
        match transcribe(blob.bytes(), filename, blob.content_type(), None, &sub).await {
            Ok(text) if text.is_empty() => {
                warn!("voice: empty transcript");
                decline("Голосовое пришло, но в нём не разобрать ни слова — пусто.".to_string()).await
            }
            Ok(text) => {
                info!(chars = text.len(), secs = ?secs, "voice: transcribed");
                // A caption (rare on voice, but possible) is context, not speech.
                match incoming.tags.get("caption").filter(|c| !c.is_empty()) {
                    Some(caption) => Some(format!("{text}\n\n(подпись к голосовому: {caption})")),
                    None => Some(text),
                }
            }
            Err(e) => {
                warn!(error = %e, "voice: transcription failed");
                decline(format!("Не смог расшифровать голосовое: {e}")).await
            }
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
        let turns = self.history.load(&channel_key).await;
        let history = to_messages(&turns);

        let base = self.prompt.base();
        let preamble = format!(
            "{base}\n\n{}\n\nCurrent time: {}\n\n{}\n\n{}\n\n{}{}",
            incoming_context(&incoming, &channel_key),
            now_rfc3339(&self.config.timezone),
            active,
            pad,
            self.skills.catalog(),
            action_context(&turns),
        );

        let (answer, restart) = self
            .run_agent(
                ctx,
                &channel_key,
                &preamble,
                input.prompt(),
                history,
                Some(incoming.source.clone()),
                is_owner(&incoming),
                feed.clone(),
            )
            .await;

        info!("→ {answer}");
        self.emit_reply(&incoming, answer.clone(), ctx).await;
        // Persist what he DID (drained from the feed) alongside what he SAID — folded
        // into the stored turn only, never into the reply the user sees.
        self.record(&channel_key, shown, with_action_log(&answer, &feed.drain_actions()))
            .await;
        // If the model asked to restart this turn, carry it out now — AFTER the reply
        // is out — so it doesn't race the teardown (the tool only recorded the intent).
        if let Some(target) = restart {
            self.apply_restart(target, ctx).await;
        }
    }

    /// Perform a restart the model requested this turn. The `restart` tool only records
    /// the target; firing it mid-turn would wind connectors down before the reply is
    /// sent (the reply would be lost). We wait until the reply has been emitted, give it
    /// a short grace to flush to the channel, then publish the control signal.
    async fn apply_restart(&self, target: String, ctx: &CogitatorContext) {
        info!(target = %target, "restart requested — flushing reply, then carrying it out");
        tokio::time::sleep(Duration::from_millis(1500)).await;
        if let Err(e) = carry_out_restart(&ctx.bus(), &self.self_source, &target).await {
            warn!(error = %e, target = %target, "failed to publish restart control signal");
        }
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
        self.emit_typing(target.clone(), Some(ChannelId::new(channel.clone())), ctx).await;
        let feed = self.feed(ctx, target.clone(), Some(ChannelId::new(channel.clone())));
        let (answer, _) = self
            .run_agent(ctx, &channel, &preamble, prompt, history, Some(target.clone()), false, feed.clone())
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
                let (out, _) = self
                    .run_agent(ctx, "system/reflection", &preamble, prompt, Vec::new(), None, false, StatusFeed::silent())
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
        reply_target: Option<ConnectorId>,
        owner: bool,
        feed: StatusFeed,
    ) -> (String, Option<String>) {
        let dispatch = OctoDispatchTool::new(ctx.bus(), self.self_source.clone(), catalog(ctx));
        let send_file =
            reply_target.map(|t| SendFileTool::new(ctx.bus(), self.self_source.clone(), t, channel));
        // Owner-only: restarting a connector (to reload its manifest) or the whole
        // process (to apply albert.toml) is an admin action. The tool records the
        // requested target here; the caller carries it out after the reply is sent.
        let pending: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let restart = owner.then(|| RestartTool::new(pending.clone()));
        // Owner-only: read/edit its own config + prompt + skill files (jailed to the
        // deploy dir, allow-listed). Applied via the restart tool above.
        let selfconfig = owner.then(|| SelfConfig::new(self.config.deploy_dir.clone()));
        let answer = match self.config.auth {
            AuthMode::ApiKey => {
                let Some(key) = self.config.api_key.as_deref() else {
                    return ("(config: api-key auth but no key loaded)".into(), None);
                };
                let client = match OpenRouterClient::new(key) {
                    Ok(c) => c,
                    Err(e) => return (format!("(llm client error: {e})"), None),
                };
                self.drive(client.agent(&self.config.model).preamble(preamble), dispatch, send_file, restart, selfconfig, channel, prompt, history, feed)
                    .await
            }
            AuthMode::Subscription => {
                // Load (and, if it's expiring, refresh) the OAuth tokens, then build
                // the Codex client for this turn.
                let sub = match ensure_fresh(&self.config.subscription_auth_json).await {
                    Ok(s) => s,
                    Err(e) => return (format!("(subscription auth: {e})"), None),
                };
                let client = match self.subscription_client(&sub) {
                    Ok(c) => c,
                    Err(e) => return (format!("(subscription auth: {e})"), None),
                };
                let model = CodexResponsesModel::make(&client, self.config.model.as_str());
                self.drive(AgentBuilder::new(model).preamble(preamble), dispatch, send_file, restart, selfconfig, channel, prompt, history, feed)
                    .await
            }
        };
        // Drain any restart the model requested during the loop (owner turns only).
        let restart_target = pending.lock().ok().and_then(|mut p| p.take());
        (answer, restart_target)
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
        send_file: Option<SendFileTool>,
        restart: Option<RestartTool>,
        selfconfig: Option<SelfConfig>,
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
            .tool(self.skills.search_tool())
            .tool(self.skills.apply_tool())
            .tool(self.skills.file_tool());
        // send_file is present only when there's a user to send to (not silent routines).
        let with_tools = match send_file {
            Some(sf) => with_tools.tool(sf),
            None => with_tools,
        };
        // restart is present only for owner turns (apply config / reboot on request).
        let with_tools = match restart {
            Some(rt) => with_tools.tool(rt),
            None => with_tools,
        };
        // self-config tools: owner turns only — read/list/write/edit its own deploy files.
        let with_tools = match selfconfig {
            Some(sc) => with_tools
                .tool(sc.read_tool())
                .tool(sc.list_tool())
                .tool(sc.write_tool())
                .tool(sc.edit_tool())
                .tool(sc.set_secret_tool())
                .tool(sc.list_secrets_tool()),
            None => with_tools,
        };
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

/// Fold this turn's action records into the assistant content we persist — so Albert's
/// transcript keeps what he DID (which tools, with which args, ok/err), not only what
/// he said. Appended to the STORED turn only, behind [`ACTION_MARKER`]; the reply the
/// user sees is untouched, and on reload the block is stripped from the assistant
/// message and surfaced via [`action_context`] instead (so it is never echoed to chat).
fn with_action_log(answer: &str, actions: &[String]) -> String {
    if actions.is_empty() {
        return answer.to_string();
    }
    let mut s = String::from(answer);
    s.push_str(ACTION_MARKER);
    for a in actions {
        s.push_str("\n- ");
        s.push_str(a);
    }
    s
}

/// Render recent action logs as a preamble section — the agent's memory of what it
/// actually did, framed as system context (like the scratchpad) so it is read, not
/// repeated. Empty string when nothing recent, so it drops cleanly out of the format.
fn action_context(turns: &[Turn]) -> String {
    match recent_actions(turns, 3) {
        Some(actions) => format!(
            "\n\nRECENT ACTIONS (system-recorded — the tools you actually ran on recent turns, \
             your ground truth for what you have already done; reference only, NEVER repeat these \
             lines in a reply):\n{actions}"
        ),
        None => String::new(),
    }
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
