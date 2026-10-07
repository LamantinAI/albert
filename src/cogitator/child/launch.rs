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

use super::super::AlbertCogitator;
use crate::{
    connector_catalog::ConnectorCatalog,
    history::{tool_trace, Turn},
    subagents::{ScopedDispatch, SubagentTool, Task},
};

impl AlbertCogitator {
    pub(super) fn spawn_child(
        self: &Arc<Self>,
        tool: &SubagentTool,
        task: Task,
    ) -> Result<Value, String> {
        if tool.ctx.shutdown.is_cancelled() {
            return Err("Runtime is shutting down.".into());
        }
        let limits = &self.config.subagents;
        if task.task.trim().is_empty() {
            return Err("Subagent task is empty.".into());
        }
        let max_turns = task.max_tool_turns.unwrap_or(limits.max_tool_turns);
        let seconds = task.timeout_secs.unwrap_or(limits.timeout_secs);
        if max_turns == 0
            || max_turns > limits.max_tool_turns
            || seconds == 0
            || seconds > limits.timeout_secs
        {
            return Err("Requested child budget exceeds configured limits.".into());
        }
        let snapshot = self.models.snapshot().scoped(&task.models)?;
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
        let available = self.child_native_tools(PathBuf::new());
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
        tool.spawned
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < limits.max_runs_per_turn).then_some(n + 1)
            })
            .map_err(|_| "Subagent run budget exhausted for this parent turn.".to_string())?;
        let (run, mut cancelled, done) = match self.children.reserve(
            limits,
            tool.parent.clone(),
            tool.conversation.clone(),
            tool.owner,
            &self.id,
        ) {
            Ok(value) => value,
            Err(error) => {
                tool.spawned.fetch_sub(1, Ordering::SeqCst);
                return Err(error);
            }
        };
        if create_dir_all(&workspace).is_err() {
            done.send_replace(
                json!({"outcome":{"status":"failed","error":"Cannot create subagent workspace."}}),
            );
            return Err("Cannot create subagent workspace.".into());
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
            .child_native_tools(workspace.clone())
            .into_iter()
            .filter(|t| task.tools.contains(&t.name()))
            .collect();
        if !connectors.is_empty() {
            let dispatch = OctoDispatchTool::new(tool.ctx.bus(), source.clone(), catalog)
                .with_scope(scope.clone())
                .with_timeout(Duration::from_secs(360))
                .with_origin(
                    ConnectorId::new(&tool.conversation.0),
                    ChannelId::new(&tool.conversation.1),
                );
            tools.push(Box::new(ScopedDispatch {
                inner: dispatch,
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
        // No await between reservation and installing the supervisor: interrupting
        // the parent cannot orphan a reserved run.
        spawn(async move {
            // Poll cancellation before starting the worker. Dropping the selected
            // future stops all its model/tool futures before we snapshot the trace.
            let outcome = {
                let execution = AssertUnwindSafe(
                    me.run_child(&owned_run, task, snapshot, tools, max_turns, &workspace),
                )
                .catch_unwind();
                select! {
                    biased;
                    _ = cancelled.changed() => json!({"status":"cancelled"}),
                    _ = ctx.shutdown.cancelled() => json!({"status":"cancelled"}),
                    _ = sleep(Duration::from_secs(seconds)) => json!({"status":"timed_out"}),
                    result = execution => match result {
                        Ok(Ok(answer)) => json!({"status":"completed","answer":answer}),
                        Ok(Err(error)) => json!({"status":"failed","error":error}),
                        Err(_) => json!({"status":"failed","error":"Subagent task stopped unexpectedly."}),
                    },
                }
            };
            if let Err(error) = carry_out_cancel(&ctx.bus(), &source, &scope).await {
                warn!(%error, %scope, "child connector cancellation failed");
            }
            let mut records = vec![Turn::user(json!({
                "run_id":scope,"parent_run_id":owned_run.parent,
                "origin_connector":owned_run.conversation.0,"origin_channel":owned_run.conversation.1,
                "assignment":info_task
            }).to_string())];
            records.extend(tool_trace(&owned_run.feed.snapshot()));
            records.push(Turn::assistant(
                json!({"outcome":outcome,"workspace":workspace}).to_string(),
            ));
            let history_key = format!("subagent/{scope}");
            let persisted = me.history.append(&history_key, &records).await.is_ok();
            info!(run_id = %scope, parent_run_id = %owned_run.parent, status = %outcome["status"], "subagent finished");
            done.send_replace(json!({"outcome":outcome,"workspace":workspace,"journal_saved":persisted,
                "note":"Check inspect for completed tools and UNKNOWN outcomes before repeating actions."}));
        });
        info!(run_id = %run.id, parent_run_id = %run.parent, "subagent spawned");
        Ok(run.view())
    }
}
