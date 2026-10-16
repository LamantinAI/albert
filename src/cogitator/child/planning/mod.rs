//! Parent-owned stage links; children only report execution outcomes.
use serde_json::{json, Value};
use tracing::debug;

use super::super::{scratchpad_scope, AlbertCogitator};
use crate::{
    models::Snapshot,
    scratchpad::{AttemptState, PadScope, ResolvedAssignment, StageLink},
    subagents::{SubagentTool, Task},
};

#[derive(Clone)]
pub(super) struct LinkedStage {
    pub scope: PadScope,
    pub link: StageLink,
    pub attempt_id: String,
}

impl AlbertCogitator {
    /// Resolve once against one owned pool snapshot, before reserving any work.
    pub(super) fn resolve_child_task(
        &self,
        tool: &SubagentTool,
        task: &mut Task,
    ) -> Result<(Snapshot, ResolvedAssignment), String> {
        let pool = self.models.snapshot();
        let resolved = if let Some(link) = &task.stage {
            let assignment = self.scratchpad.stage_assignment(
                &scratchpad_scope(&tool.conversation, tool.owner),
                &link.task_id,
                &link.stage_id,
            )?;
            let canonical = pool.resolve_child(
                assignment.role.as_deref(),
                &assignment.models,
                assignment.reasoning_effort.as_deref(),
            )?;
            if (!task.models.is_empty() && task.models != canonical.models)
                || (task.role.is_some() && task.role != canonical.role)
                || (task.reasoning_effort.is_some()
                    && task.reasoning_effort != canonical.reasoning_effort)
            {
                return Err(
                    "Task model/role/effort overrides conflict with the stage assignment.".into(),
                );
            }
            canonical
        } else {
            pool.resolve_child(
                task.role.as_deref(),
                &task.models,
                task.reasoning_effort.as_deref(),
            )?
        };
        let snapshot = pool.child_snapshot(&resolved)?;
        task.models = resolved.models.clone();
        task.role = resolved.role.clone();
        task.reasoning_effort = resolved.reasoning_effort.clone();
        Ok((snapshot, resolved))
    }

    pub(super) fn existing_stage_launch(
        &self,
        tool: &SubagentTool,
        attempt_id: String,
        run_id: Option<String>,
        state: AttemptState,
    ) -> Value {
        let mut view = run_id.as_deref()
            .and_then(|id| self.children.get(id, &tool.conversation, tool.owner).ok())
            .map(|run| run.view())
            .unwrap_or_else(|| json!({"run_id":run_id,"result_available":false,
                "note":"This attempt already exists. If its run is no longer retained or launch failed, inspect the stage; use a new launch key and current revision for a deliberate new attempt."}));
        view["attempt_id"] = json!(attempt_id);
        view["attempt_state"] = json!(state);
        view["already_launched"] = json!(true);
        view
    }

    pub(super) fn release_stage_launch(&self, stage: &Option<LinkedStage>) {
        if let Some(stage) = stage {
            let _ = self.scratchpad.release_reservation(
                &stage.scope,
                &stage.link.task_id,
                &stage.link.stage_id,
                &stage.attempt_id,
            );
        }
    }

    pub(super) fn record_stage_model(&self, stage: &LinkedStage, model: String) {
        if let Err(error) = self.scratchpad.append_actual_model(
            &stage.scope,
            &stage.link.task_id,
            &stage.link.stage_id,
            &stage.attempt_id,
            model,
        ) {
            // Clearing/replacing a task intentionally detaches its old callbacks.
            debug!(%error, "stage model callback was not retained");
        }
    }

    pub(super) fn finish_stage_launch(&self, stage: &LinkedStage, outcome: &Value) {
        let state = match outcome["status"].as_str() {
            Some("completed") => AttemptState::Completed,
            Some("partial") => AttemptState::Partial,
            Some("cancelled") => AttemptState::Cancelled,
            Some("timed_out") => AttemptState::TimedOut,
            _ => AttemptState::Failed,
        };
        if let Err(error) = self.scratchpad.finish_attempt(
            &stage.scope,
            &stage.link.task_id,
            &stage.link.stage_id,
            &stage.attempt_id,
            state,
            outcome["status"].as_str().map(str::to_owned),
        ) {
            debug!(%error, "stage outcome callback was not retained");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs::write,
        sync::{atomic::Ordering, Arc},
        time::Duration,
    };

    use octo_core::{
        CogitatorContext, ConnectorCapabilities, ConnectorId, ConnectorInfo, EventBus, EventKind,
        Filter, SubscribeOptions,
    };
    use octo_rig::carry_out_cancel;
    use rig::tool::Tool;
    use serde_json::{from_value, json, to_value, Value};
    use tempfile::tempdir;
    use tokio::time::timeout;

    use crate::{
        cogitator::{provider_fixture::setup, scratchpad_scope, AlbertCogitator},
        models::{ModelPool, ReasoningSupport, ReasoningWire},
        scratchpad::{PlanArgs, Status},
        subagents::{Args, SubagentTool, Task},
    };

    fn tool(host: &Arc<AlbertCogitator>, ctx: CogitatorContext) -> SubagentTool {
        SubagentTool::new(
            Arc::downgrade(host),
            ctx,
            ("telegram".into(), "room".into()),
            "parent-1".into(),
            true,
        )
    }
    fn read(host: &AlbertCogitator, tool: &SubagentTool) -> Value {
        host.scratchpad.plan_action(
            &scratchpad_scope(&tool.conversation, tool.owner),
            from_value::<PlanArgs>(json!({"action":"get"})).unwrap(),
        )
    }
    fn mutate(host: &AlbertCogitator, tool: &SubagentTool, value: Value) -> Value {
        host.scratchpad.plan_action(
            &scratchpad_scope(&tool.conversation, tool.owner),
            from_value(value).unwrap(),
        )
    }
    async fn planned(host: &AlbertCogitator, tool: &SubagentTool, assignment: Value) -> Task {
        let scope = scratchpad_scope(&tool.conversation, tool.owner);
        host.scratchpad
            .scoped_handle(scope)
            .goal()
            .call(from_value(json!({"goal":"Research and verify"})).unwrap())
            .await
            .unwrap();
        let pad = read(host, tool);
        let stage = mutate(
            host,
            tool,
            json!({"action":"add","task_id":pad["task_id"],
            "expected_revision":pad["revision"],"text":"Research a question",
            "spec":{"assignment":assignment,"acceptance":["Checked the findings"]}}),
        );
        assert_eq!(stage["ok"], true, "{stage}");
        from_value(json!({"task":"Research a question", "stage":{
            "task_id":stage["task_id"],"stage_id":stage["stage_id"],
            "expected_revision":stage["revision"],"launch_key":"research-1"}}))
        .unwrap()
    }
    async fn wait(host: &Arc<AlbertCogitator>, tool: &SubagentTool, id: &Value) -> Value {
        timeout(
            Duration::from_secs(5),
            host.subagent_command(
                tool,
                Args::Wait {
                    run_id: id.as_str().unwrap().into(),
                    seconds: 4,
                },
            ),
        )
        .await
        .unwrap()
        .unwrap()
    }

    #[tokio::test]
    async fn staged_launch_deduplicates_and_parent_verifies_acceptance() {
        let (mut host, ctx, server) = setup("healthy").await;
        let me = Arc::get_mut(&mut host).unwrap();
        let pool = me.config.models.as_mut().unwrap();
        pool.models[0].reasoning = Some(ReasoningSupport {
            wire: ReasoningWire::ReasoningObject,
            supported_efforts: vec!["low".into()],
        });
        pool.roles[0].reasoning_effort = Some("low".into());
        me.models = ModelPool::new(&me.config);
        let tool = tool(&host, ctx);
        let task = planned(&host, &tool, json!({"role":"test_models"})).await;
        let link = task.stage.clone().unwrap();
        let first = host
            .subagent_command(&tool, Args::Spawn { task: task.clone() })
            .await
            .unwrap();
        let result = wait(&host, &tool, &first["run_id"]).await;
        assert_eq!(result["result"]["outcome"]["status"], "completed");
        let second = host
            .subagent_command(&tool, Args::Spawn { task })
            .await
            .unwrap();
        assert_eq!(second["run_id"], first["run_id"]);
        assert_eq!(second["already_launched"], true);
        assert_eq!(tool.spawned.load(Ordering::SeqCst), 1);
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        assert_eq!(
            server.requests.lock().unwrap()[0]["reasoning"]["effort"],
            "low"
        );
        let stage = host
            .scratchpad
            .get_stage(
                &scratchpad_scope(&tool.conversation, true),
                &link.task_id,
                &link.stage_id,
            )
            .unwrap();
        assert_eq!(stage.status, Status::InProgress);
        assert_eq!(stage.attempts[0].actual_models, ["healthy"]);
        assert_eq!(
            stage.attempts[0].assignment.reasoning_effort.as_deref(),
            Some("low")
        );
        assert_eq!(
            stage.attempts[0].run_id.as_deref(),
            first["run_id"].as_str()
        );
        let pad = read(&host, &tool);
        let rejected = mutate(
            &host,
            &tool,
            json!({"action":"mark","task_id":link.task_id,
            "expected_revision":pad["revision"],"stage_id":link.stage_id,"status":"verified"}),
        );
        assert_eq!(rejected["ok"], false);
        let done = mutate(
            &host,
            &tool,
            json!({"action":"mark","task_id":link.task_id,
            "expected_revision":pad["revision"],"stage_id":link.stage_id,"status":"done"}),
        );
        assert_eq!(done["ok"], true);
        let verified = mutate(
            &host,
            &tool,
            json!({"action":"mark","task_id":link.task_id,
            "expected_revision":done["revision"],"stage_id":link.stage_id,"status":"verified",
            "evidence":[{"criterion":0,"reference":first["run_id"],"summary":"Checked findings"}]}),
        );
        assert_eq!(verified["ok"], true);
    }

    #[tokio::test]
    async fn default_routing_is_primary_only_and_switch_affects_new_children() {
        let (mut host, ctx, server) = setup("failing").await;
        let me = Arc::get_mut(&mut host).unwrap();
        me.config.models.as_mut().unwrap().roles.clear();
        me.models = ModelPool::new(&me.config);
        let tool = tool(&host, ctx);
        let cap = host
            .subagent_command(&tool, Args::Capabilities)
            .await
            .unwrap();
        assert_eq!(cap["default_models"], json!(["failing"]));
        assert_eq!(cap["models"].as_array().unwrap().len(), 1);
        assert_eq!(cap["models"][0]["id"], "failing");
        let denied: Task = from_value(json!({"task":"do work","models":["healthy"]})).unwrap();
        assert!(host
            .subagent_command(&tool, Args::Spawn { task: denied })
            .await
            .is_err());
        assert!(server.requests.lock().unwrap().is_empty());
        let task = || from_value(json!({"task":"do work"})).unwrap();
        let first = host
            .subagent_command(&tool, Args::Spawn { task: task() })
            .await
            .unwrap();
        assert_eq!(
            wait(&host, &tool, &first["run_id"]).await["result"]["outcome"]["status"],
            "failed"
        );
        assert!(server
            .requests
            .lock()
            .unwrap()
            .iter()
            .all(|r| r["model"] == "failing"));
        host.models.command("healthy", true);
        let next = host
            .subagent_command(&tool, Args::Spawn { task: task() })
            .await
            .unwrap();
        assert_eq!(
            wait(&host, &tool, &next["run_id"]).await["result"]["outcome"]["status"],
            "completed"
        );
        assert_eq!(
            server.requests.lock().unwrap().last().unwrap()["model"],
            "healthy"
        );
        assert_eq!(
            host.children
                .get(first["run_id"].as_str().unwrap(), &tool.conversation, true)
                .unwrap()
                .status(),
            "failed"
        );
    }

    #[tokio::test]
    async fn conflicting_assignment_stale_revision_and_other_scope_never_launch() {
        let (host, ctx, server) = setup("failing").await;
        let tool = tool(&host, ctx);
        let task = planned(&host, &tool, json!({"models":["healthy"]})).await;
        let mut conflict = task.clone();
        conflict.models.push("failing".into());
        assert!(host
            .subagent_command(&tool, Args::Spawn { task: conflict })
            .await
            .is_err());
        let mut stale = task.clone();
        stale.stage.as_mut().unwrap().expected_revision -= 1;
        assert!(host
            .subagent_command(&tool, Args::Spawn { task: stale })
            .await
            .is_err());
        let mut outsider = tool.clone();
        outsider.owner = false;
        assert!(host
            .subagent_command(&outsider, Args::Spawn { task: task.clone() })
            .await
            .is_err());
        outsider.owner = true;
        outsider.conversation.0 = "another-connector".into();
        assert!(host
            .subagent_command(&outsider, Args::Spawn { task })
            .await
            .is_err());
        assert!(server.requests.lock().unwrap().is_empty());
        assert!(host.children.visible(&tool.conversation, true).is_empty());
        assert_eq!(tool.spawned.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn failed_prelaunch_releases_registry_budget_and_stage_reservation() {
        let (mut host, ctx, server) = setup("healthy").await;
        let dir = tempdir().unwrap();
        let file = dir.path().join("file");
        write(&file, "not a directory").unwrap();
        Arc::get_mut(&mut host).unwrap().config.code_workspace = file;
        let tool = tool(&host, ctx);
        let task = planned(&host, &tool, json!({})).await;
        let link = task.stage.clone().unwrap();
        assert!(host
            .subagent_command(&tool, Args::Spawn { task: task.clone() })
            .await
            .is_err());
        assert!(host.children.visible(&tool.conversation, true).is_empty());
        assert_eq!(tool.spawned.load(Ordering::SeqCst), 0);
        assert!(server.requests.lock().unwrap().is_empty());
        let recovered = host
            .subagent_command(&tool, Args::Spawn { task })
            .await
            .unwrap();
        assert_eq!(recovered["already_launched"], true);
        assert_eq!(recovered["attempt_state"], "failed");
        assert!(recovered["run_id"].is_null());
        let stage = host
            .scratchpad
            .get_stage(
                &scratchpad_scope(&tool.conversation, true),
                &link.task_id,
                &link.stage_id,
            )
            .unwrap();
        assert_eq!(stage.attempts.len(), 1);
    }

    #[tokio::test]
    async fn parent_interrupt_recovers_same_run_and_explicit_cancel_updates_attempt() {
        let (host, ctx, _server) = setup("child-dispatch").await;
        let ctx = CogitatorContext::new(
            ctx.shutdown.clone(),
            ctx.bus(),
            vec![ConnectorInfo {
                id: ConnectorId::new("search"),
                capabilities: ConnectorCapabilities {
                    description: Some("search.web: Search the web".into()),
                    event_kinds_accept: vec![EventKind::new("search.web")],
                    ..Default::default()
                },
            }],
        );
        let mut commands = ctx
            .bus()
            .subscribe(Filter::by_kind("search.web"), SubscribeOptions::default())
            .await
            .unwrap();
        let tool = tool(&host, ctx.clone());
        let mut task = planned(&host, &tool, json!({})).await;
        task.connectors.push("search".into());
        let link = task.stage.clone().unwrap();
        let first = host
            .subagent_command(&tool, Args::Spawn { task: task.clone() })
            .await
            .unwrap();
        timeout(Duration::from_secs(3), commands.next())
            .await
            .unwrap()
            .unwrap();
        carry_out_cancel(&ctx.bus(), &host.self_source, &tool.parent)
            .await
            .unwrap();
        let mut resumed = tool.clone();
        resumed.parent = "parent-after-interrupt".into();
        let recovered = host
            .subagent_command(&resumed, Args::Spawn { task })
            .await
            .unwrap();
        assert_eq!(recovered["run_id"], first["run_id"]);
        assert_eq!(recovered["attempt_state"], "running");
        assert!(host.cancel_channel(&tool.conversation, &ctx).await);
        let stage = host
            .scratchpad
            .get_stage(
                &scratchpad_scope(&tool.conversation, true),
                &link.task_id,
                &link.stage_id,
            )
            .unwrap();
        assert_eq!(stage.status, Status::InProgress);
        assert_eq!(to_value(stage.attempts[0].state).unwrap(), "cancelled");
        assert_eq!(stage.attempts[0].actual_models, ["child-dispatch"]);
    }
}
