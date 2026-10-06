use std::{
    env::var,
    sync::{Arc, Mutex},
    time::Duration,
};

use octo_code::code_tools;
use octo_core::{CogitatorContext, ConnectorId};
use octo_openai_auth::Subscription as SubToken;
use octo_rig::{OctoDispatchTool, RestartTool, SendFileTool};
use rig::{
    agent::{AgentBuilder, NoToolConfig},
    client::CompletionClient,
    completion::{CompletionModel, Message, Prompt, PromptError},
    http_client::{HeaderMap, HeaderValue, ReqwestClient},
    providers::{openai, openrouter::Client as OpenRouterClient},
};
use tracing::{debug, info};

use super::{catalog, AlbertCogitator};
use crate::{
    codex_http::CodexHttp,
    codex_model::CodexResponsesModel,
    cogitator::errors::model_failure,
    config::AuthMode,
    history::with_call_ids,
    models::{needs_vision, Failure, FailureKind, ModelSpec},
    selfconfig::SelfConfig,
    status::StatusFeed,
};

impl AlbertCogitator {
    /// Build the LLM client per the configured auth mode, then run one rig
    /// tool-loop. The two modes yield different concrete model types, so the
    /// build-tools-and-run tail lives in the generic [`Self::drive`].
    pub(super) async fn run_agent(
        &self,
        ctx: &CogitatorContext,
        channel: &str,
        preamble: &str,
        prompt: Message,
        history: Vec<Message>,
        reply_target: Option<ConnectorId>,
        owner: bool,
        feed: StatusFeed,
        scope: Option<&str>,
    ) -> (String, Option<String>) {
        // Owner-only: restarting a connector (to reload its manifest) or the whole
        // process (to apply albert.toml) is an admin action. The tool records the
        // requested target here; the caller carries it out after the reply is sent.
        let pending: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        // One drive run consumes its tool instances, and the forced-refresh retry
        // below needs a second set — so the tools are built per attempt.
        let make_tools = || {
            // How long the rig tool waits for a connector's reply. octo-rig defaults to 20s,
            // which is BELOW what a skill may legitimately run: forkd's ceiling is 300s
            // (max_timeout_secs), and the imagegen / transcribe skills request up to it — so
            // at the default the rig would abort the await while the script was still running
            // and the model would report a phantom "response timeout". The invariant is
            // script < forkd < rig: this ceiling must sit ABOVE forkd's max (300s), so forkd
            // (which knows the job) owns the kill. 360s = 300 + margin. Fast connectors
            // (calendar/jira/storage/search/browser) answer far under it and never come near.
            let mut dispatch =
                OctoDispatchTool::new(ctx.bus(), self.self_source.clone(), catalog(ctx))
                    .with_timeout(Duration::from_secs(360));
            // Stamp this turn's cancellation scope on every dispatch, so a /cancel can
            // reach the connector work (forkd scripts) the model starts this turn.
            if let Some(s) = scope {
                dispatch = dispatch.with_scope(s);
            }
            let send_file = reply_target
                .clone()
                .map(|t| SendFileTool::new(ctx.bus(), self.self_source.clone(), t, channel));
            let restart = owner.then(|| RestartTool::new(pending.clone()));
            // Owner-only: read/edit its own config + prompt + skill files (jailed to the
            // deploy dir, allow-listed). Applied via the restart tool above.
            let selfconfig = owner.then(|| SelfConfig::new(self.config.deploy_dir.clone()));
            (dispatch, send_file, restart, selfconfig)
        };
        let snapshot = self.models.snapshot();
        let vision = needs_vision(history.iter().chain([&prompt]));
        // Hearing is already complete and is part of the supplied history, not
        // an action to repeat. Only new model-generated tool rounds block retry.
        let baseline = feed.tool_call_count();
        let answer = snapshot
            .run(
                vision,
                |model, force_refresh| {
                    let tools = make_tools();
                    let history = with_call_ids(history.clone());
                    let prompt = prompt.clone();
                    let feed = feed.clone();
                    async move {
                        self.model_attempt(
                            &model,
                            force_refresh,
                            preamble,
                            tools,
                            channel,
                            prompt,
                            history,
                            feed,
                        )
                        .await
                    }
                },
                || feed.tool_call_count() > baseline,
                |message| {
                    info!(channel, status = %message, "model pool");
                    let feed = feed.clone();
                    async move {
                        feed.model_status(message).await;
                    }
                },
            )
            .await
            .unwrap_or_else(|error| error);
        // Drain any restart the model requested during the loop (owner turns only).
        let restart_target = pending.lock().ok().and_then(|mut p| p.take());
        (answer, restart_target)
    }

    async fn model_attempt(
        &self,
        model: &ModelSpec,
        force_refresh: bool,
        preamble: &str,
        tools: TurnTools,
        channel: &str,
        prompt: Message,
        history: Vec<Message>,
        feed: StatusFeed,
    ) -> Result<String, Failure> {
        let unavailable = |message: &str| Failure {
            kind: FailureKind::Unavailable,
            message: message.into(),
        };
        let http = ReqwestClient::builder()
            .timeout(Duration::from_millis(model.request_timeout_ms))
            .build()
            .map_err(|_| unavailable("Could not construct HTTP client."))?;
        let (dispatch, send_file, restart, selfconfig) = tools;
        let result = match model.provider {
            AuthMode::ApiKey => {
                let key = match &model.api_key_env {
                    Some(name) => var(name).ok(),
                    None => self.config.api_key.clone(),
                }
                .filter(|key| !key.trim().is_empty())
                .ok_or_else(|| unavailable("API key is unavailable."))?;
                let mut builder = OpenRouterClient::builder()
                    .api_key(key.as_str())
                    .http_client(http);
                let base = model.base_url.as_deref().unwrap_or(&self.config.base_url);
                if !base.is_empty() {
                    builder = builder.base_url(base);
                }
                let client = builder
                    .build()
                    .map_err(|_| unavailable("Could not construct API client."))?;
                self.drive(
                    client.agent(&model.model).preamble(preamble),
                    dispatch,
                    send_file,
                    restart,
                    selfconfig,
                    channel,
                    prompt,
                    history,
                    feed,
                )
                .await
            }
            AuthMode::Subscription => {
                let sub = if force_refresh {
                    self.auth.force_refresh().await
                } else {
                    self.auth.fresh().await
                }
                .map_err(|_| {
                    unavailable("Subscription authentication failed; check albert login.")
                })?;
                let client = self
                    .subscription_client(
                        &sub,
                        model
                            .base_url
                            .as_deref()
                            .unwrap_or(&self.config.subscription_base_url),
                        http,
                    )
                    .map_err(|_| unavailable("Could not construct subscription client."))?;
                let model = CodexResponsesModel::make(&client, &model.model);
                self.drive(
                    AgentBuilder::new(model).preamble(preamble),
                    dispatch,
                    send_file,
                    restart,
                    selfconfig,
                    channel,
                    prompt,
                    history,
                    feed,
                )
                .await
            }
        }
        .map_err(model_failure)?;
        if result.trim().is_empty() {
            return Err(Failure {
                kind: FailureKind::Incompatible,
                message: "Model returned an empty answer.".into(),
            });
        }
        Ok(result)
    }

    /// A ChatGPT-subscription rig client: rig's OpenAI provider (Responses API by
    /// default) pointed at the Codex backend, with the OAuth access token as the
    /// bearer and the account id in the mandatory `ChatGPT-Account-ID` header.
    pub(super) fn subscription_client(
        &self,
        sub: &SubToken,
        base_url: &str,
        http: ReqwestClient,
    ) -> Result<openai::Client<CodexHttp>, String> {
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
            .base_url(base_url)
            .api_key(sub.access_token.as_str())
            .http_headers(headers)
            .http_client(CodexHttp::with_client(http))
            .build()
            .map_err(|e| format!("client build: {e}"))
    }

    /// Attach the toolset (kaeru memory + dispatch + scratchpad + skills) to a
    /// fresh agent builder and run one bounded tool-loop. Generic over the model
    /// so both auth modes share it; `install` takes the no-tools-yet builder, so
    /// it runs before the dispatch/scratchpad tools are chained on. Returns the
    /// raw loop error so the caller can decide (retry a revoked-token 401, or
    /// classify it for the pool).
    pub(super) async fn drive<M>(
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
    ) -> Result<String, PromptError>
    where
        M: CompletionModel + 'static,
    {
        let pad = self.scratchpad.handle(channel);
        debug!(
            channel,
            clouds = !self.config.clouds.is_empty(),
            max_turns = self.config.max_tool_turns,
            "building agent + running tool-loop"
        );
        let installed = self.memory.install(base);
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
                .tool(self.models.select_tool())
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
        agent
            .prompt(prompt)
            .with_hook(feed)
            .max_turns(self.config.max_tool_turns)
            .with_history(history)
            .await
    }
}

/// The per-attempt tool instances for one drive run (dispatch + the optional
/// send-file / owner-only restart and self-config tools). A run consumes them, so
/// the forced-refresh retry in [`AlbertCogitator::run_agent`] builds a fresh set.
type TurnTools = (
    OctoDispatchTool,
    Option<SendFileTool>,
    Option<RestartTool>,
    Option<SelfConfig>,
);
