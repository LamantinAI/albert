mod entry;
use std::{
    env::var,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use octo_approval::Ingress;
use octo_code::{EditTool, GlobTool, GrepTool, ListTool, ReadTool, WriteTool};
use octo_core::{ChannelId, CogitatorContext, ConnectorId};
use octo_openai_auth::Subscription as SubToken;
use octo_rig::{OctoDispatchTool, RestartTool, SendFileTool};
use rig::{
    agent::{AgentBuilder, NoToolConfig},
    client::CompletionClient,
    completion::{CompletionModel, Message, Prompt, PromptError},
    http_client::{HeaderMap, HeaderValue, ReqwestClient},
    providers::{openai, openrouter::Client as OpenRouterClient},
    tool::ToolDyn,
};
use tracing::{debug, info, warn};

use super::{
    catalog,
    retention::{fence_tools, permitted_memory, temporary},
    scratchpad_scope,
    session_dispatch::SessionDispatch,
    AlbertCogitator,
};
use crate::{
    artifacts::Artifacts,
    codex_http::CodexHttp,
    codex_model::CodexResponsesModel,
    cogitator::errors::model_failure,
    config::AuthMode,
    connector_catalog::ConnectorCatalog,
    context::BudgetedModel,
    models::{needs_vision, Failure, FailureKind, ModelSpec},
    openrouter_http::OpenRouterHttp,
    scratchpad::PadScope,
    selfconfig::SelfConfig,
    sessions::{telegram::output_scope, SessionContext},
    status::StatusFeed,
    subagents::{budget::Budget, SubagentTool},
};

impl AlbertCogitator {
    pub(super) async fn run_agent_scoped(
        self: &Arc<Self>,
        ctx: &CogitatorContext,
        channel: &str,
        preamble: &str,
        prompt: Message,
        history: Vec<Message>,
        reply_target: Option<ConnectorId>,
        owner: bool,
        feed: StatusFeed,
        scope: Option<&str>,
        ingress: Option<Ingress>,
        session: Option<SessionContext>,
    ) -> (String, Option<String>) {
        if session.as_ref().is_some_and(|session| !session.is_live()) {
            return ("Session ended.".into(), None);
        }
        let ingress =
            match super::retention::scope_ingress(&self.authority, ingress, session.as_ref()) {
                Ok(ingress) => ingress,
                Err(_) => return ("Session authority is unavailable.".into(), None),
            };
        let host = Arc::downgrade(self);
        let conversation = Self::logical_conversation(
            channel,
            reply_target.as_ref().map(|target| target.as_str()),
        );
        let physical_channel = session
            .as_ref()
            .map(|session| session.reply_route().channel())
            .unwrap_or(channel);
        let observer_conversation = conversation.clone();
        let feed = feed.with_result_observer(move |name, args, result| {
            if let Some(host) = host.upgrade() {
                host.acknowledge_subagent_result(&observer_conversation, owner, name, args, result);
            }
        });
        // Owner-only: restarting a connector (to reload its manifest) or the whole
        // process (to apply albert.toml) is an admin action. The tool records the
        // requested target here; the caller carries it out after the reply is sent.
        let pending: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        // One drive run consumes its tool instances, and the forced-refresh retry
        // below needs a second set — so the tools are built per attempt.
        let discovery = ConnectorCatalog::new(ctx.connectors());
        let child_tool = self.config.subagents.enabled.then(|| {
            SubagentTool::new(
                Arc::downgrade(self),
                ctx.clone(),
                conversation.clone(),
                scope.unwrap_or("routine").into(),
                owner,
            )
            .with_ingress(ingress.clone())
            .with_session(session.clone())
        });
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
            if let Some(target) = &reply_target {
                dispatch = dispatch
                    .with_channel_for(target.clone(), ChannelId::new(physical_channel))
                    .with_origin(target.clone(), ChannelId::new(physical_channel));
            }
            if let Some(session) = &session {
                dispatch = dispatch
                    .with_context(session.clone())
                    .with_context(output_scope(session, scope.map(str::to_owned)));
            }
            // Stamp this turn's cancellation scope on every dispatch, so a /cancel can
            // reach the connector work (forkd scripts) the model starts this turn.
            if let Some(s) = scope {
                dispatch = dispatch.with_scope(s);
            }
            if let Some(ingress) = ingress.as_ref().filter(|p| self.authority.recognizes(p)) {
                let targets = ctx
                    .connectors()
                    .iter()
                    .filter(|c| {
                        c.capabilities
                            .event_kinds_accept
                            .iter()
                            .any(|kind| kind.as_str().starts_with("sql."))
                    })
                    .map(|c| c.id.clone())
                    .collect();
                dispatch =
                    dispatch.with_authority(self.authority.clone(), ingress.clone(), targets);
            }
            let send_file = reply_target.clone().map(|t| {
                let confirmed = ctx.connectors().iter().any(|c| {
                    c.id == t
                        && c.capabilities
                            .event_kinds_emit
                            .iter()
                            .any(|k| k.as_str() == "chat.send_file.result")
                });
                let tool = SendFileTool::new(
                    ctx.bus(),
                    self.self_source.clone(),
                    t.clone(),
                    physical_channel,
                )
                .with_origin(t, ChannelId::new(physical_channel));
                let tool = match &session {
                    Some(session) => tool
                        .with_context(session.clone())
                        .with_context(output_scope(session, scope.map(str::to_owned))),
                    None => tool,
                };
                if confirmed {
                    tool.with_confirmation_timeout(Duration::from_secs(120))
                } else {
                    tool
                }
            });
            let restart = owner.then(|| RestartTool::new(pending.clone()));
            // Owner-only: read/edit its own config + prompt + skill files (jailed to the
            // deploy dir, allow-listed). Applied via the restart tool above.
            let selfconfig = owner.then(|| SelfConfig::new(self.config.deploy_dir.clone()));
            AttemptTools::Root((
                SessionDispatch {
                    inner: dispatch,
                    session: session.clone(),
                },
                send_file,
                restart,
                selfconfig,
                discovery.clone(),
                child_tool.clone(),
                Artifacts::new(
                    self.config.code_workspace.clone(),
                    &conversation,
                    owner,
                    None,
                    self.config.context.artifacts.clone(),
                ),
                scratchpad_scope(&conversation, owner),
            ))
        };
        let preamble = format!("{preamble}{}", self.children.context(&conversation, owner));
        let preamble = preamble.as_str();
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
                    let history = history.clone();
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
                |_message| async move {
                    info!(channel, "model pool state changed");
                },
            )
            .await
            .unwrap_or_else(|error| {
                if temporary(session.as_ref()) && error.contains("context budget") {
                    format!("{error}\nSave this chat before continuing with a larger tool result.")
                } else {
                    error
                }
            });
        // Drain any restart the model requested during the loop (owner turns only).
        let restart_target = pending.lock().ok().and_then(|mut p| p.take());
        (answer, restart_target)
    }

    pub(super) async fn model_attempt(
        &self,
        model: &ModelSpec,
        force_refresh: bool,
        preamble: &str,
        tools: AttemptTools,
        channel: &str,
        prompt: Message,
        history: Vec<Message>,
        feed: StatusFeed,
    ) -> Result<String, Failure> {
        if !feed.is_live() {
            return Err(Failure {
                kind: FailureKind::Fatal,
                message: "Session ended.".into(),
            });
        }
        let unavailable = |message: &str| Failure {
            kind: FailureKind::Unavailable,
            message: message.into(),
        };
        let reasoning_params = model.reasoning_params().map_err(|message| Failure {
            kind: FailureKind::Incompatible,
            message,
        })?;
        let turn_budget = match &tools {
            AttemptTools::Child { budget, .. } => Some(budget.clone()),
            _ => None,
        };
        let compact = matches!(&tools, AttemptTools::Compact { .. });
        let timeout_ms = if compact {
            self.config.context.request_timeout_ms
        } else {
            model.request_timeout_ms
        };
        let started = Instant::now();
        let http = ReqwestClient::builder()
            .timeout(Duration::from_millis(timeout_ms))
            .build()
            .map_err(|_| unavailable("Could not construct HTTP client."))?;

        let mut budget = self.config.context.for_model(model);
        if let AttemptTools::Compact { max_tokens } = &tools {
            budget.response_tokens = *max_tokens;
        }
        let budget = budget.enabled.then_some(budget);
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
                    .http_client(OpenRouterHttp::new(http));
                let base = model.base_url.as_deref().unwrap_or(&self.config.base_url);
                if !base.is_empty() {
                    builder = builder.base_url(base);
                }
                let client = builder
                    .build()
                    .map_err(|_| unavailable("Could not construct API client."))?;
                self.drive_attempt(
                    AgentBuilder::new(
                        BudgetedModel::new(client.completion_model(&model.model), budget, true)
                            .with_model_params(reasoning_params)
                            .with_turn_budget(turn_budget)
                            .with_recovery(
                                self.config.context.continuation_retries,
                                self.config.context.continuation_retry_delay_ms,
                                feed.clone(),
                            ),
                    )
                    .preamble(preamble),
                    tools,
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
                self.drive_attempt(
                    AgentBuilder::new(
                        BudgetedModel::new(model, budget, false)
                            .with_model_params(reasoning_params)
                            .with_turn_budget(turn_budget)
                            .with_recovery(
                                self.config.context.continuation_retries,
                                self.config.context.continuation_retry_delay_ms,
                                feed.clone(),
                            ),
                    )
                    .preamble(preamble),
                    tools,
                    channel,
                    prompt,
                    history,
                    feed,
                )
                .await
            }
        }
        .map_err(|error| {
            let failure = model_failure(error);
            warn!(model_id=%model.id, compact, timeout_ms, elapsed_ms=started.elapsed().as_millis(),
                kind=?failure.kind, "model attempt failed");
            failure
        })?;
        if result.trim().is_empty() {
            return Err(Failure {
                kind: FailureKind::Incompatible,
                message: "Model returned an empty answer.".into(),
            });
        }
        Ok(result)
    }

    async fn drive_attempt<M: CompletionModel + 'static>(
        &self,
        base: AgentBuilder<M, (), NoToolConfig>,
        tools: AttemptTools,
        channel: &str,
        prompt: Message,
        history: Vec<Message>,
        feed: StatusFeed,
    ) -> Result<String, PromptError> {
        match tools {
            AttemptTools::Root((
                dispatch,
                send_file,
                restart,
                selfconfig,
                discovery,
                children,
                artifacts,
                pad_scope,
            )) => {
                self.drive(
                    base, dispatch, send_file, restart, selfconfig, discovery, children, channel,
                    prompt, history, feed, artifacts, pad_scope,
                )
                .await
            }
            AttemptTools::Compact { max_tokens } => {
                base.max_tokens(max_tokens as u64)
                    .build()
                    .prompt(prompt)
                    .with_history(history)
                    .max_turns(1)
                    .await
            }
            AttemptTools::Child {
                tools, max_turns, ..
            } => {
                base.tools(tools)
                    .build()
                    .prompt(prompt)
                    .with_history(history)
                    .with_hook(feed)
                    .max_turns(max_turns)
                    .await
            }
        }
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
        dispatch: SessionDispatch,
        send_file: Option<SendFileTool>,
        restart: Option<RestartTool>,
        selfconfig: Option<SelfConfig>,
        discovery: ConnectorCatalog,
        children: Option<SubagentTool>,
        channel: &str,
        prompt: Message,
        history: Vec<Message>,
        feed: StatusFeed,
        artifacts: Artifacts,
        pad_scope: PadScope,
    ) -> Result<String, PromptError>
    where
        M: CompletionModel + 'static,
    {
        let pad = self.scratchpad.scoped_handle(pad_scope);
        debug!(
            channel,
            clouds = !self.config.clouds.is_empty(),
            max_turns = self.config.max_tool_turns,
            "building agent + running tool-loop"
        );
        let session = dispatch.session.clone();
        let volatile = temporary(session.as_ref());
        let mut tools = permitted_memory(self.memory.delegation_tools(), volatile);
        tools.extend(vec![
            Box::new(dispatch) as Box<dyn ToolDyn>,
            Box::new(discovery),
            Box::new(pad.goal()),
            Box::new(pad.step()),
            Box::new(pad.mark()),
            Box::new(pad.note()),
            Box::new(pad.clear()),
            Box::new(pad.plan()),
            Box::new(self.skills.list_tool()),
            Box::new(self.skills.search_tool()),
            Box::new(self.skills.apply_tool()),
            Box::new(self.skills.file_tool()),
            Box::new(ReadTool),
            Box::new(WriteTool),
            Box::new(EditTool),
            Box::new(ListTool),
            Box::new(GlobTool),
            Box::new(GrepTool),
        ]);
        if let Some(tool) = send_file {
            tools.push(Box::new(tool));
        }
        if let Some(tool) = restart {
            tools.push(Box::new(tool));
        }
        if let Some(sc) = selfconfig {
            tools.extend(vec![
                Box::new(self.models.select_tool()) as Box<dyn ToolDyn>,
                Box::new(sc.read_tool()),
                Box::new(sc.list_tool()),
                Box::new(sc.write_tool()),
                Box::new(sc.edit_tool()),
                Box::new(sc.set_secret_tool()),
                Box::new(sc.list_secrets_tool()),
            ]);
        }
        if let Some(tool) = children {
            tools.push(Box::new(tool));
        }
        let artifacts = if volatile {
            artifacts.without_disk()
        } else {
            artifacts
        };
        let agent = base
            .tools(fence_tools(artifacts.wrap(tools), session))
            .build();
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
    SessionDispatch,
    Option<SendFileTool>,
    Option<RestartTool>,
    Option<SelfConfig>,
    ConnectorCatalog,
    Option<SubagentTool>,
    Artifacts,
    PadScope,
);

pub(super) enum AttemptTools {
    Compact {
        max_tokens: usize,
    },
    Root(TurnTools),
    Child {
        tools: Vec<Box<dyn ToolDyn>>,
        max_turns: usize,
        budget: Arc<Budget>,
    },
}
