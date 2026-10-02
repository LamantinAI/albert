use std::{
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
    http_client::{HeaderMap, HeaderValue},
    providers::{openai, openrouter::Client as OpenRouterClient},
};
use tokio::time::sleep;
use tracing::{debug, info, warn};

use super::{
    catalog, llm_error, token_rejected, transient, AlbertCogitator, TRANSIENT_BACKOFF_SECS,
};
use crate::{
    codex_http::CodexHttp, codex_model::CodexResponsesModel, config::AuthMode,
    selfconfig::SelfConfig, status::StatusFeed,
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
        let answer = match self.config.auth {
            AuthMode::ApiKey => {
                let Some(key) = self.config.api_key.as_deref() else {
                    return ("(config: api-key auth but no key loaded)".into(), None);
                };
                let client = match OpenRouterClient::new(key) {
                    Ok(c) => c,
                    Err(e) => return (format!("(llm client error: {e})"), None),
                };
                let (dispatch, send_file, restart, selfconfig) = make_tools();
                self.drive(
                    client.agent(&self.config.model).preamble(preamble),
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
                .unwrap_or_else(llm_error)
            }
            AuthMode::Subscription => {
                // Load (and, if it's expiring, refresh) the OAuth tokens, then build
                // the Codex client for this turn.
                let sub = match self.auth.fresh().await {
                    Ok(s) => s,
                    Err(e) => return (format!("(subscription auth: {e})"), None),
                };
                // Two different failures both want the turn run again, so the attempt
                // lives in a loop rather than in two hand-written retries.
                let mut sub = sub;
                let mut refreshed = false;
                let mut hiccups = 0usize;
                loop {
                    let attempt = self
                        .subscription_attempt(
                            &sub,
                            preamble,
                            make_tools(),
                            channel,
                            prompt.clone(),
                            history.clone(),
                            feed.clone(),
                        )
                        .await;
                    match attempt {
                        Ok(answer) => break answer,
                        // The server can revoke an access token ahead of its JWT `exp`
                        // (live incident 2026-08-10: 401 token_expired with exp on 08-18),
                        // and `ensure_fresh` trusts `exp` — so without this the turn (and
                        // every turn after it) would 401 forever. Force the refresh and
                        // retry the whole turn once; a rejected refresh surfaces the auth
                        // error, which already points at `albert login`.
                        Err(e) if token_rejected(&e) && !refreshed && !feed.has_tool_calls() => {
                            warn!(error = %e, "access token rejected live; forcing refresh and retrying the turn");
                            match self.auth.force_refresh().await {
                                Ok(fresh) => {
                                    // The aborted attempt may have recorded a restart
                                    // target in `pending`; clear it so only what the retried
                                    // turn actually asks for is carried out — otherwise a
                                    // restart requested by the failed attempt leaks into an
                                    // otherwise-clean retry and fires unbidden.
                                    let _ = pending.lock().map(|mut p| p.take());
                                    sub = fresh;
                                    refreshed = true;
                                    continue;
                                }
                                Err(e) => break format!("(subscription auth: {e})"),
                            }
                        }
                        // A busy provider is not an answer. Upstream returns
                        // `server_is_overloaded` in bursts (live: 5 of 13 turns on
                        // 2026-08-27), and handing that straight to the user turns a
                        // hiccup lasting seconds into a failed request. Wait and run the
                        // turn again; the user sees the delay, not the error.
                        Err(e)
                            if transient(&e)
                                && hiccups < TRANSIENT_BACKOFF_SECS.len()
                                && !feed.has_tool_calls() =>
                        {
                            let wait = TRANSIENT_BACKOFF_SECS[hiccups];
                            warn!(error = %e, attempt = hiccups + 1, wait_s = wait, "provider hiccup; retrying the turn");
                            let _ = pending.lock().map(|mut p| p.take());
                            sleep(Duration::from_secs(wait)).await;
                            hiccups += 1;
                            continue;
                        }
                        Err(e) => break llm_error(e),
                    }
                }
            }
        };
        // Drain any restart the model requested during the loop (owner turns only).
        let restart_target = pending.lock().ok().and_then(|mut p| p.take());
        (answer, restart_target)
    }

    /// One subscription-mode attempt: build the per-turn Codex client from `sub` and
    /// run the tool-loop. A client-build failure is config-shaped and no retry can
    /// fix it, so it lands in `Ok` as the final user-facing answer; `Err` is the live
    /// tool-loop error, which the caller inspects for a revoked-token 401.
    pub(super) async fn subscription_attempt(
        &self,
        sub: &SubToken,
        preamble: &str,
        tools: TurnTools,
        channel: &str,
        prompt: Message,
        history: Vec<Message>,
        feed: StatusFeed,
    ) -> Result<String, PromptError> {
        let client = match self.subscription_client(sub) {
            Ok(c) => c,
            Err(e) => return Ok(format!("(subscription auth: {e})")),
        };
        let model = CodexResponsesModel::make(&client, self.config.model.as_str());
        let (dispatch, send_file, restart, selfconfig) = tools;
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

    /// A ChatGPT-subscription rig client: rig's OpenAI provider (Responses API by
    /// default) pointed at the Codex backend, with the OAuth access token as the
    /// bearer and the account id in the mandatory `ChatGPT-Account-ID` header.
    pub(super) fn subscription_client(
        &self,
        sub: &SubToken,
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
    /// it runs before the dispatch/scratchpad tools are chained on. Returns the
    /// raw loop error so the caller can decide (retry a revoked-token 401, or
    /// render it via [`llm_error`]).
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
