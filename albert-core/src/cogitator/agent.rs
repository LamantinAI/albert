mod attempt;
mod chat;
mod entry;
mod provider;
mod tools;
mod types;
pub(super) use self::types::AttemptTools;
use self::{entry::root_diagnostics, types::TurnTools};
use std::{future::pending as never, sync::Arc, sync::Mutex, time::Duration};

use octo_approval::Ingress;
use octo_core::{ChannelId, CogitatorContext, ConnectorId};
use octo_rig::{OctoDispatchTool, RestartTool, SendFileTool};
use rig::completion::Message;
use tokio::select;
use tracing::info;

use super::{
    catalog, media_delivery::MediaDelivery, outcome::AgentOutcome, retention::scope_ingress,
    retention::temporary, scratchpad_scope, session_dispatch::SessionDispatch, AlbertCogitator,
};
use crate::{
    artifacts::Artifacts, connector_catalog::ConnectorCatalog, messaging::contract::WorkRef,
    models::needs_vision, selfconfig::SelfConfig, sessions::output::output_scope,
    sessions::SessionContext, status::StatusFeed, subagents::SubagentTool,
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
        logical_work: Option<WorkRef>,
    ) -> (AgentOutcome, Option<String>) {
        if let Some(session) = &session {
            if !session.authorized().await {
                return (AgentOutcome::Failed("Account access changed.".into()), None);
            }
        }
        let chat_only = session.as_ref().is_some_and(SessionContext::chat_only);
        let ingress = if chat_only {
            None
        } else {
            match scope_ingress(&self.authority, ingress, session.as_ref()) {
                Ok(ingress) => ingress,
                Err(_) => {
                    return (
                        AgentOutcome::Failed("Session authority is unavailable.".into()),
                        None,
                    )
                }
            }
        };
        let feed = feed.with_diagnostics(Some(root_diagnostics(
            session.as_ref(),
            logical_work.as_ref(),
            scope,
            reply_target.is_some(),
        )));
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
        let progress_store = self.scratchpad.clone();
        let progress_scope = scratchpad_scope(&conversation, owner);
        let feed = feed.with_stage_source(move || progress_store.progress_scoped(&progress_scope));
        feed.register_connectors(ctx.connectors());
        // Owner-only: restarting a connector (to reload its manifest) or the whole
        // process (to apply albert.toml) is an admin action. The tool records the
        // requested target here; the caller carries it out after the reply is sent.
        let pending: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        // One drive run consumes its tool instances, and the forced-refresh retry
        // below needs a second set — so the tools are built per attempt.
        let discovery = (!chat_only).then(|| ConnectorCatalog::new(ctx.connectors()));
        let mcp = self
            .mcp
            .as_ref()
            .and_then(|registry| registry.snapshot(session.as_ref()).ok())
            .unwrap_or_default();
        let child_tool = (!chat_only && self.config.subagents.enabled).then(|| {
            SubagentTool::new(
                Arc::downgrade(self),
                ctx.clone(),
                conversation.clone(),
                scope.unwrap_or("routine").into(),
                owner,
            )
            .with_ingress(ingress.clone())
            .with_session(session.clone())
            .with_diagnostics(feed.diagnostics())
            .with_progress(feed.clone())
            .with_mcp(mcp.clone())
        });
        let make_tools = || {
            if chat_only {
                return AttemptTools::ChatOnly {
                    session: session.clone(),
                    pad: scratchpad_scope(&conversation, false),
                };
            }
            // Keep the tool RPC ceiling above forkd's 300-second skill limit.
            // The connector owns termination; 360 seconds leaves response margin
            // and avoids reporting a phantom timeout while scripts still run.
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
                let tool = if confirmed {
                    tool.with_confirmation_timeout(Duration::from_secs(120))
                } else {
                    tool
                };
                MediaDelivery::new(
                    tool,
                    self.session_service.clone(),
                    session.clone(),
                    self.config.code_workspace.clone(),
                    reply_target.as_ref().is_some_and(|t| t.as_str() == "web"),
                )
            });
            let restart = owner.then(|| RestartTool::new(pending.clone()));
            // Owner-only: read/edit its own config + prompt + skill files (jailed to the
            // deploy dir, allow-listed). Applied via the restart tool above.
            let selfconfig = owner.then(|| SelfConfig::new(self.config.deploy_dir.clone()));
            AttemptTools::Root(TurnTools {
                dispatch: SessionDispatch {
                    inner: dispatch,
                    session: session.clone(),
                    messaging: self.messaging_tools(
                        ctx,
                        session.as_ref(),
                        logical_work
                            .clone()
                            .or_else(|| scope.map(|scope| WorkRef::Turn(scope.into()))),
                    ),
                },
                send_file,
                restart,
                selfconfig,
                discovery: discovery.as_ref().expect("host tool policy").clone(),
                children: child_tool.clone(),
                artifacts: Artifacts::new(
                    self.config.code_workspace.clone(),
                    &conversation,
                    owner,
                    None,
                    self.config.context.artifacts.clone(),
                ),
                pad_scope: scratchpad_scope(&conversation, owner),
                credential: self.request_credential_tool(session.as_ref(), scope, ingress.clone()),
                mcp: mcp.clone(),
                confirmation: self.request_confirmation_tool(
                    session.as_ref(),
                    scope,
                    ingress.clone(),
                    feed.clone(),
                ),
            })
        };
        let children = if chat_only {
            String::new()
        } else {
            self.children.context(&conversation, owner)
        };
        let preamble = format!("{preamble}{children}");
        let preamble = preamble.as_str();
        let snapshot = self.models.snapshot();
        let vision = needs_vision(history.iter().chain([&prompt]));
        // Hearing is already complete and is part of the supplied history, not
        // an action to repeat. Only new model-generated tool rounds block retry.
        let baseline = feed.tool_call_count();
        let execution = Box::pin(snapshot.run(
            vision,
            |model, force_refresh| {
                let tools = make_tools();
                let history = history.clone();
                let prompt = prompt.clone();
                let feed = feed.clone();
                async move {
                    self.model_attempt_outcome(
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
        ));
        let answer = select! {
            biased;
            _ = async { match &session { Some(s) => s.access_cancelled().await, None => never::<()>().await } } => {
                if let Some(scope) = scope { self.cancel_scope(scope, ctx).await; }
                return (AgentOutcome::Failed("Account access changed.".into()), None);
            },
            result = execution => result,
        }.unwrap_or_else(|error| AgentOutcome::Failed(
            if temporary(session.as_ref()) && error.contains("context budget") {
                format!("{error}\nSave this chat before continuing with a larger tool result.")
            } else { error }
        ));
        // Drain any restart the model requested during the loop (owner turns only).
        let restart_target = pending.lock().ok().and_then(|mut p| p.take());
        (answer, restart_target)
    }
}
