use std::{
    collections::HashSet,
    fs::create_dir_all,
    panic::AssertUnwindSafe,
    path::PathBuf,
    sync::{atomic::Ordering, Arc},
    time::Duration,
};

use futures::FutureExt;
use octo_core::{ChannelId, ConnectorId};
use octo_rig::{carry_out_cancel, OctoDispatchTool};
use rand::random;
use serde_json::{json, Value};
use tokio::{select, spawn, time::sleep};
use tracing::{info, warn};

use super::{
    super::{scratchpad_scope, AlbertCogitator},
    planning::LinkedStage,
};
use crate::{
    connector_catalog::ConnectorCatalog,
    context::DiagnosticScope,
    history::{tool_trace, Turn},
    scratchpad::Reservation,
    sessions::telegram::output_scope,
    subagents::{ScopedDispatch, SubagentTool, Task},
};

impl AlbertCogitator {
    pub(super) fn spawn_child(
        self: &Arc<Self>,
        tool: &SubagentTool,
        mut task: Task,
    ) -> Result<Value, String> {
        if tool.ctx.shutdown.is_cancelled() {
            return Err("Runtime is shutting down.".into());
        }
        if tool
            .session
            .as_ref()
            .is_some_and(|session| !session.is_live())
        {
            return Err("Session ended; child was not started.".into());
        }
        if tool
            .session
            .as_ref()
            .is_some_and(|session| session.chat_only())
        {
            return Err("This account cannot delegate work.".into());
        }
        let limits = &self.config.subagents;
        if task.task.trim().is_empty() {
            return Err("Subagent task is empty.".into());
        }
        let max_turns = task
            .max_tool_turns
            .unwrap_or(limits.default_tool_turns.min(limits.max_tool_turns));
        let seconds = task.timeout_secs.unwrap_or(limits.timeout_secs);
        if max_turns == 0
            || max_turns > limits.max_tool_turns
            || seconds == 0
            || seconds > limits.timeout_secs
        {
            return Err("Requested child budget exceeds configured limits.".into());
        }
        let (snapshot, assignment) = self.resolve_child_task(tool, &mut task)?;
        if tool.ingress.is_none()
            && (!task.sql_stores.is_empty() || task.sql_write || task.sql_schema)
        {
            return Err("SQL delegation requires authenticated parent grants.".into());
        }
        let child_ingress = tool
            .ingress
            .as_ref()
            .map(|ingress| {
                ingress
                    .attenuate(task.sql_stores.clone(), task.sql_write, task.sql_schema)
                    .map(|child| child.limit_lifetime(Duration::from_secs(seconds)))
            })
            .transpose()?;
        let allowed: HashSet<_> = task.connectors.iter().cloned().collect();
        let connectors: Vec<_> = tool
            .ctx
            .connectors()
            .iter()
            .filter(|c| allowed.contains(c.id.as_str()) && c.capabilities.description.is_some())
            .cloned()
            .collect();
        if connectors.len() != allowed.len() {
            return Err("Unknown or unadvertised connector grant.".into());
        }
        let available = self.child_tools_for(PathBuf::new(), tool.session.as_ref());
        for name in &task.tools {
            if !available.iter().any(|t| t.name() == *name) {
                return Err(format!("Native tool cannot be delegated: {name}"));
            }
        }
        let workspace = self
            .config
            .code_workspace
            .join("subagents")
            .join(format!("{:032x}", random::<u128>()));
        let stage = if let Some(link) = &task.stage {
            let scope = scratchpad_scope(&tool.conversation, tool.owner);
            match self.scratchpad.reserve_stage(&scope, link, assignment)? {
                Reservation::New { attempt_id } => Some(LinkedStage {
                    scope,
                    link: link.clone(),
                    attempt_id,
                }),
                Reservation::Existing {
                    attempt_id,
                    run_id,
                    state,
                } => {
                    return Ok(self.existing_stage_launch(tool, attempt_id, run_id, state));
                }
            }
        } else {
            None
        };
        if tool
            .spawned
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < limits.max_runs_per_turn).then_some(n + 1)
            })
            .is_err()
        {
            self.release_stage_launch(&stage);
            return Err("Subagent run budget exhausted for this parent turn.".into());
        }
        let (run, mut cancelled, _done) = match self.children.reserve_scoped(
            limits,
            tool.parent.clone(),
            tool.conversation.clone(),
            tool.owner,
            &self.id,
            tool.session.clone(),
        ) {
            Ok(value) => value,
            Err(error) => {
                tool.spawned.fetch_sub(1, Ordering::SeqCst);
                self.release_stage_launch(&stage);
                return Err(error);
            }
        };
        run.feed.set_diagnostics(Some(DiagnosticScope {
            lane: "child",
            session_id: run
                .session
                .as_ref()
                .map(|session| session.session_id().as_str().into()),
            work_id: tool
                .diagnostics
                .as_ref()
                .and_then(|scope| scope.work_id.clone()),
            attempt_id: run.id.clone(),
            run_id: Some(run.id.clone()),
        }));
        if create_dir_all(&workspace).is_err() {
            self.children.release_unstarted(&run);
            tool.spawned.fetch_sub(1, Ordering::SeqCst);
            self.release_stage_launch(&stage);
            return Err("Cannot create subagent workspace.".into());
        }
        if let Some(stage) = &stage {
            if let Err(error) = self.scratchpad.bind_run(
                &stage.scope,
                &stage.link.task_id,
                &stage.link.stage_id,
                &stage.attempt_id,
                &run.id,
            ) {
                self.children.release_unstarted(&run);
                tool.spawned.fetch_sub(1, Ordering::SeqCst);
                self.release_stage_launch(&Some(stage.clone()));
                return Err(error);
            }
        }
        let scope = run.id.clone();
        let source = ConnectorId::new(format!("{}/{}", self.self_source, scope));
        let catalog = connectors
            .iter()
            .map(|c| {
                format!(
                    "- target \"{}\":\n{}",
                    c.id,
                    c.capabilities.description.as_deref().unwrap_or("")
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let mut tools: Vec<_> = self
            .child_tools_for(workspace.clone(), tool.session.as_ref())
            .into_iter()
            .filter(|t| task.tools.contains(&t.name()))
            .collect();
        if !connectors.is_empty() {
            let mut dispatch = OctoDispatchTool::new(tool.ctx.bus(), source.clone(), catalog)
                .with_scope(scope.clone())
                .with_timeout(Duration::from_secs(360))
                .with_origin(
                    ConnectorId::new(
                        tool.session
                            .as_ref()
                            .map(|s| s.reply_route().connector())
                            .unwrap_or(&tool.conversation.0),
                    ),
                    ChannelId::new(
                        tool.session
                            .as_ref()
                            .map(|s| s.reply_route().channel())
                            .unwrap_or(&tool.conversation.1),
                    ),
                );
            if let Some(session) = &tool.session {
                dispatch = dispatch
                    .with_context(session.clone())
                    .with_context(output_scope(session, Some(scope.clone())));
            }
            if let Some(child) = child_ingress {
                let targets = connectors
                    .iter()
                    .filter(|c| {
                        c.capabilities
                            .event_kinds_accept
                            .iter()
                            .any(|kind| kind.as_str().starts_with("sql."))
                    })
                    .map(|c| c.id.clone())
                    .collect();
                dispatch = dispatch.with_authority(self.authority.clone(), child, targets);
            }
            tools.push(Box::new(ScopedDispatch {
                inner: dispatch,
                session: tool.session.clone(),
                allowed: connectors
                    .iter()
                    .map(|c| (c.id.to_string(), c.capabilities.event_kinds_accept.clone()))
                    .collect(),
            }));
            tools.push(Box::new(ConnectorCatalog::new(&connectors)));
        }
        let me = self.clone();
        let ctx = tool.ctx.clone();
        let owned_run = run.clone();
        let info_task = task.clone();
        let physical_origin = tool
            .session
            .as_ref()
            .map(|session| {
                (
                    session.reply_route().connector().to_owned(),
                    session.reply_route().channel().to_owned(),
                )
            })
            .unwrap_or_else(|| tool.conversation.clone());
        let session_id = tool
            .session
            .as_ref()
            .map(|session| session.session_id().clone());
        // No await between reservation and installing the supervisor: interrupting
        // the parent cannot orphan a reserved run.
        spawn(async move {
            // Poll cancellation before starting the worker. Dropping the selected
            // future stops all its model/tool futures before we snapshot the trace.
            let outcome = {
                let execution = AssertUnwindSafe(me.run_child(
                    &owned_run,
                    task,
                    snapshot,
                    tools,
                    max_turns,
                    &workspace,
                    stage.as_ref(),
                ))
                .catch_unwind();
                select! {
                    biased;
                    _ = cancelled.changed() => json!({"status":"cancelled"}),
                    _ = ctx.shutdown.cancelled() => json!({"status":"cancelled"}),
                    _ = sleep(Duration::from_secs(seconds)) => json!({"status":"timed_out"}),
                    result = execution => match result {
                        Ok(Ok(answer)) => json!({"status":if answer.budget_exhausted {"partial"} else {"completed"},
                            "answer":answer.text,"tool_rounds":answer.tool_rounds,"tool_round_limit":max_turns,
                            "budget_exhausted":answer.budget_exhausted}),
                        Ok(Err(error)) => json!({"status":"failed","error":error}),
                        Err(_) => json!({"status":"failed","error":"Subagent task stopped unexpectedly."}),
                    },
                }
            };
            if let Some(stage) = &stage {
                me.finish_stage_launch(stage, &outcome);
            }
            if let Err(error) = carry_out_cancel(&ctx.bus(), &source, &scope).await {
                warn!(%error, %scope, "child connector cancellation failed");
            }
            let mut records = vec![Turn::user(
                json!({
                    "run_id":scope,"parent_run_id":owned_run.parent,
                    "origin_connector":physical_origin.0,"origin_channel":physical_origin.1,
                    "logical_conversation":owned_run.conversation,"session_id":session_id,
                    "assignment":info_task
                })
                .to_string(),
            )];
            records.extend(tool_trace(&owned_run.feed.snapshot()));
            records.push(Turn::assistant(
                json!({"outcome":outcome,"workspace":workspace}).to_string(),
            ));
            let history_key = owned_run.history_key();
            let persisted = me
                .append_host(&history_key, &scope, "child-final", records)
                .await
                .is_ok();
            info!(run_id = %scope, parent_run_id = %owned_run.parent, status = %outcome["status"], "subagent finished");
            owned_run.complete(json!({"outcome":outcome,"workspace":workspace,"journal_saved":persisted,
                "note":"Check inspect for completed tools and UNKNOWN outcomes before repeating actions."}));
        });
        info!(run_id = %run.id, parent_run_id = %run.parent, "subagent spawned");
        Ok(run.view())
    }
}
