mod launch;
mod native;
mod runner;

// Assembly policy for one-level delegation. The Octo bus remains generic.
use std::{path::PathBuf, sync::Arc};

use rig::completion::Message;
use serde_json::{from_str, json, Value};

use super::AlbertCogitator;
use crate::{
    history::{journal_messages, to_messages},
    subagents::{
        inspection::{inspect, read_entry},
        Args, Conversation, Run, SubagentTool,
    },
};

impl AlbertCogitator {
    pub(crate) async fn subagent_command(
        self: &Arc<Self>,
        tool: &SubagentTool,
        args: Args,
    ) -> Result<Value, String> {
        if !self.config.subagents.enabled {
            return Err("Subagents are disabled.".into());
        }
        match args {
            Args::Capabilities => Ok(json!({
                "models":self.models.snapshot().config.models.iter().map(|m|
                    json!({"id":m.id,"tools":m.tools,"vision":m.vision})).collect::<Vec<_>>(),
                "connectors":tool.ctx.connectors().iter().filter(|c| c.capabilities.description.is_some())
                    .map(|c| c.id.as_str()).collect::<Vec<_>>(),
                "tools":self.child_native_tools(PathBuf::new()).iter().map(|t| t.name()).collect::<Vec<_>>(),
                "max_concurrent":self.config.subagents.max_concurrent,
                "max_tool_turns":self.config.subagents.max_tool_turns,
                "default_tool_turns":self.config.subagents.default_tool_turns.min(self.config.subagents.max_tool_turns),
                "timeout_secs":self.config.subagents.timeout_secs,
                "max_depth":1
            })),
            Args::Spawn { task } => self.spawn_child(tool, task),
            Args::List => Ok(
                json!({"runs":self.children.visible(&tool.conversation, tool.owner)
                .iter().map(|run| run.summary()).collect::<Vec<_>>()}),
            ),
            Args::Inspect { run_id, offset } => {
                let run = self.children.get(&run_id, &tool.conversation, tool.owner)?;
                let messages = self.inspection_messages(&run).await;
                let index = inspect(
                    &messages,
                    &self.config.code_workspace.join("tool-results"),
                    &self.config.subagents.inspection,
                    offset,
                );
                Ok(json!({"run":run.summary(),"journal":index}))
            }
            Args::Read {
                run_id,
                entry,
                part,
                field,
                offset,
                limit,
            } => {
                let run = self.children.get(&run_id, &tool.conversation, tool.owner)?;
                let messages = self.inspection_messages(&run).await;
                read_entry(
                    &messages,
                    entry,
                    &part,
                    &field,
                    offset,
                    limit,
                    &self.config.subagents.inspection,
                )
            }
            Args::Wait { run_id, seconds } => {
                let run = self.children.get(&run_id, &tool.conversation, tool.owner)?;
                let result = run.wait(seconds).await;
                Ok(result)
            }
            Args::Cancel { run_id } => {
                let run = self.children.get(&run_id, &tool.conversation, tool.owner)?;
                let _ = run.cancel.send(true);
                Ok(run.wait(60).await)
            }
        }
    }
    pub(super) fn acknowledge_subagent_result(
        &self,
        conversation: &Conversation,
        owner: bool,
        name: &str,
        args: &str,
        result: &str,
    ) {
        if name != "subagent" {
            return;
        }
        let (Ok(args), Ok(result)) = (from_str::<Value>(args), from_str::<Value>(result)) else {
            return;
        };
        if args["action"] != "wait" || result["result"].is_null() {
            return;
        }
        let Some(id) = args["run_id"].as_str() else {
            return;
        };
        if result["run_id"].as_str() != Some(id) {
            return;
        }
        if let Ok(run) = self.children.get(id, conversation, owner) {
            run.acknowledge();
        }
    }

    async fn inspection_messages(&self, run: &Run) -> Vec<Message> {
        let persisted = run
            .result
            .borrow()
            .get("journal_saved")
            .and_then(Value::as_bool)
            == Some(true);
        if persisted {
            to_messages(&self.history.load(&format!("subagent/{}", run.id)).await)
        } else {
            // Running or unpersisted work remains available, without pretending a
            // failed database write succeeded. Never write a journal file.
            journal_messages(&run.feed.snapshot())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::provider_fixture::setup;
    use super::*;
    use crate::{status::StatusFeed, subagents::Task};
    use octo_core::{
        CogitatorContext, ConnectorCapabilities, ConnectorId, ConnectorInfo, Envelope, EventBus,
        EventKind, Filter, SubscribeOptions,
    };
    use octo_rig::carry_out_cancel;
    use rig::completion::Message;
    use serde_json::{from_value, json, Value};
    use std::{sync::Arc, time::Duration};
    use tokio::{spawn, time::timeout};

    fn tool(host: &Arc<AlbertCogitator>, ctx: CogitatorContext) -> SubagentTool {
        SubagentTool::new(
            Arc::downgrade(host),
            ctx,
            ("telegram".into(), "room".into()),
            "parent-1".into(),
            true,
        )
    }
    fn task(model: &str) -> Task {
        from_value(json!({"task":"A child task","context":"Explicit materials","models":[model]}))
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
    fn connectors(ctx: &CogitatorContext) -> CogitatorContext {
        CogitatorContext::new(
            ctx.shutdown.clone(),
            ctx.bus(),
            vec![
                ConnectorInfo {
                    id: ConnectorId::new("search"),
                    capabilities: ConnectorCapabilities {
                        description: Some("search.web {query}: Search the web".into()),
                        event_kinds_accept: vec![EventKind::new("search.web")],
                        ..Default::default()
                    },
                },
                ConnectorInfo {
                    id: ConnectorId::new("hidden"),
                    capabilities: ConnectorCapabilities {
                        description: Some("secret.read: A secret connector".into()),
                        ..Default::default()
                    },
                },
            ],
        )
    }

    #[tokio::test]
    async fn real_root_tool_spawns_child_without_inheriting_history_or_tools() {
        let (host, ctx, server) = setup("delegates").await;
        let feed = StatusFeed::silent();
        timeout(
            Duration::from_secs(5),
            host.run_agent(
                &ctx,
                "room",
                "root persona secret",
                Message::user("delegate"),
                vec![Message::user("parent history secret")],
                Some(ConnectorId::new("telegram")),
                true,
                feed.clone(),
                Some("root-run"),
            ),
        )
        .await
        .unwrap();
        let tool = tool(&host, ctx);
        let runs = host.children.visible(&tool.conversation, true);
        assert_eq!(runs.len(), 1);
        let result = wait(&host, &tool, &json!(runs[0].id)).await;
        assert_eq!(result["result"]["outcome"]["answer"], "answer from healthy");
        assert_eq!(result["parent_run_id"], "root-run");
        assert_eq!(host.models.snapshot().selected, "delegates");
        let requests = server.requests.lock().unwrap();
        let child = requests.iter().find(|r| r["model"] == "healthy").unwrap();
        assert!(child["tools"].is_null() || child["tools"].as_array().is_some_and(Vec::is_empty));
        let wire = child.to_string();
        assert!(wire.contains("Only this explicit context"));
        assert!(!wire.contains("parent history secret"));
        assert!(!wire.contains("root persona secret"));
        assert!(!wire.contains("config_set_secret"));
    }

    #[tokio::test]
    async fn fallback_is_confined_and_denied_native_grants_never_start_work() {
        let (host, ctx, server) = setup("failing").await;
        let tool = tool(&host, ctx);
        for name in ["model_select", "restart", "config_read", "subagent"] {
            let mut task = task("healthy");
            task.tools.push(name.into());
            assert!(host
                .subagent_command(&tool, Args::Spawn { task })
                .await
                .is_err());
        }
        assert!(server.requests.lock().unwrap().is_empty());
        let first = host
            .subagent_command(
                &tool,
                Args::Spawn {
                    task: task("failing"),
                },
            )
            .await
            .unwrap();
        let failed = wait(&host, &tool, &first["run_id"]).await;
        assert_eq!(failed["result"]["outcome"]["status"], "failed");
        assert!(!server
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|r| r["model"] == "healthy"));
        let mut both = task("failing");
        both.models.push("healthy".into());
        let second = host
            .subagent_command(&tool, Args::Spawn { task: both })
            .await
            .unwrap();
        assert_eq!(
            wait(&host, &tool, &second["run_id"]).await["result"]["outcome"]["status"],
            "completed"
        );
        assert_eq!(host.models.snapshot().selected, "failing");
    }

    #[tokio::test]
    async fn granted_connector_executes_and_hidden_catalog_is_absent() {
        let (host, ctx, server) = setup("child-dispatch").await;
        let ctx = connectors(&ctx);
        let mut incoming = ctx
            .bus()
            .subscribe(Filter::by_kind("search.web"), SubscribeOptions::default())
            .await
            .unwrap();
        let bus = ctx.bus();
        let responder = spawn(async move {
            let command = timeout(Duration::from_secs(3), incoming.next())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(command.channel, None, "no implicit parent destination");
            assert!(command.source.as_str().contains("child-"));
            let response = Envelope::new(
                ConnectorId::new("search"),
                EventKind::new("search.web.result"),
                json!({"text":"found"}),
            )
            .with_target(command.source.clone())
            .with_correlation(command.id);
            bus.publish(response).await.unwrap();
        });
        let tool = tool(&host, ctx);
        let mut task = task("child-dispatch");
        task.connectors.push("search".into());
        let started = host
            .subagent_command(&tool, Args::Spawn { task })
            .await
            .unwrap();
        let result = wait(&host, &tool, &started["run_id"]).await;
        assert_eq!(result["result"]["outcome"]["status"], "completed");
        responder.await.unwrap();
        let requests = server.requests.lock().unwrap();
        let wire = requests[0]["tools"].to_string();
        assert!(wire.contains("search.web"));
        assert!(!wire.contains("secret.read"));
        assert!(!wire.contains("kaeru_"));
        assert!(!wire.contains("scratchpad"));
        assert!(!wire.contains("subagent"));
    }

    #[tokio::test]
    async fn parent_interrupt_leaves_child_running_cancel_reaches_child_and_journals_unknown() {
        let (host, ctx, _server) = setup("child-dispatch").await;
        let ctx = connectors(&ctx);
        let mut commands = ctx
            .bus()
            .subscribe(Filter::by_kind("search.web"), SubscribeOptions::default())
            .await
            .unwrap();
        let mut cancellations = ctx
            .bus()
            .subscribe(
                Filter::by_kind("octo.control.cancel"),
                SubscribeOptions::default(),
            )
            .await
            .unwrap();
        let tool = tool(&host, ctx.clone());
        let mut task = task("child-dispatch");
        task.connectors.push("search".into());
        let started = host
            .subagent_command(&tool, Args::Spawn { task })
            .await
            .unwrap();
        timeout(Duration::from_secs(3), commands.next())
            .await
            .unwrap()
            .unwrap();
        // Parent interruption sends only its own scope, not descendant scopes.
        carry_out_cancel(&ctx.bus(), &host.self_source, &tool.parent)
            .await
            .unwrap();
        let root_cancel = timeout(Duration::from_secs(2), cancellations.next())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(root_cancel.payload_as::<String>().unwrap(), "parent-1");
        let run = host
            .children
            .get(
                started["run_id"].as_str().unwrap(),
                &tool.conversation,
                true,
            )
            .unwrap();
        assert!(run.result.borrow().is_null());
        assert!(host.cancel_channel(&tool.conversation, &ctx).await);
        let cancellation = timeout(Duration::from_secs(2), cancellations.next())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(cancellation.payload_as::<String>().unwrap(), &run.id);
        assert_eq!(run.result.borrow()["outcome"]["status"], "cancelled");
        let journal = host
            .subagent_command(
                &tool,
                Args::Inspect {
                    run_id: run.id.clone(),
                    offset: 0,
                },
            )
            .await
            .unwrap()
            .to_string();
        assert!(journal.contains("UNKNOWN"));
        assert!(journal.contains("search.web"));
        assert_eq!(
            host.history
                .load(&format!("subagent/{}", run.id))
                .await
                .len(),
            3
        );
    }
    #[tokio::test]
    async fn native_tools_are_explicit_and_child_tool_effects_stop_fallback() {
        let (host, ctx, server) = setup("writes").await;
        let tool = tool(&host, ctx);
        let mut child = task("writes");
        child.models.push("healthy".into());
        child.tools.push("scratchpad_note".into());
        let started = host
            .subagent_command(&tool, Args::Spawn { task: child })
            .await
            .unwrap();
        let result = wait(&host, &tool, &started["run_id"]).await;
        assert_eq!(result["result"]["outcome"]["status"], "failed");
        assert!(result.to_string().contains("tools may already have run"));
        let requests = server.requests.lock().unwrap();
        assert!(requests.iter().all(|r| r["model"] == "writes"));
        let tools = requests[0]["tools"].as_array().unwrap();
        let mut names: Vec<_> = tools
            .iter()
            .map(|t| t["function"]["name"].as_str().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, ["artifact", "scratchpad_note"]);
        assert!(!host.scratchpad.render("room").contains("recorded once"));
    }

    #[tokio::test]
    async fn deadline_and_spawn_budget_bound_child_work() {
        let (host, ctx, _server) = setup("child-dispatch").await;
        let ctx = connectors(&ctx);
        let tool = tool(&host, ctx);
        let mut child = task("child-dispatch");
        child.connectors.push("search".into());
        child.timeout_secs = Some(1);
        let started = host
            .subagent_command(&tool, Args::Spawn { task: child })
            .await
            .unwrap();
        let result = wait(&host, &tool, &started["run_id"]).await;
        assert_eq!(result["result"]["outcome"]["status"], "timed_out");
        tool.spawned.store(
            host.config.subagents.max_runs_per_turn,
            std::sync::atomic::Ordering::SeqCst,
        );
        assert!(host
            .subagent_command(
                &tool,
                Args::Spawn {
                    task: task("healthy")
                }
            )
            .await
            .unwrap_err()
            .contains("budget"));
        let mut excessive = task("healthy");
        excessive.max_tool_turns = Some(host.config.subagents.max_tool_turns + 1);
        assert!(host
            .subagent_command(&tool, Args::Spawn { task: excessive })
            .await
            .unwrap_err()
            .contains("limits"));
    }
    #[tokio::test]
    async fn completed_inspection_reads_history_and_enforces_run_access() {
        let (host, ctx, _server) = setup("writes").await;
        let tool = tool(&host, ctx);
        let mut child = task("writes");
        child.tools.push("scratchpad_note".into());
        let started = host
            .subagent_command(&tool, Args::Spawn { task: child })
            .await
            .unwrap();
        wait(&host, &tool, &started["run_id"]).await;
        let id = started["run_id"].as_str().unwrap().to_owned();
        let run = host.children.get(&id, &tool.conversation, true).unwrap();
        run.feed.checkpoint(); // Drop live trace: persisted journal is authoritative.
        let view = host
            .subagent_command(
                &tool,
                Args::Inspect {
                    run_id: id.clone(),
                    offset: 0,
                },
            )
            .await
            .unwrap();
        assert_eq!(view["journal"]["entries"][0]["tool"], "scratchpad_note");
        assert!(view["run"].get("result").is_none());
        let args = || Args::Read {
            run_id: id.clone(),
            entry: 0,
            part: "arguments".into(),
            field: vec!["text".into()],
            offset: 0,
            limit: None,
        };
        let read = host.subagent_command(&tool, args()).await.unwrap();
        assert_eq!(read["content"], "recorded once");
        let mut outsider = tool.clone();
        outsider.conversation.1 = "another-room".into();
        assert!(host.subagent_command(&outsider, args()).await.is_err());
        outsider.conversation = tool.conversation.clone();
        outsider.owner = false;
        assert!(host.subagent_command(&outsider, args()).await.is_err());
    }
    #[tokio::test]
    async fn exhausted_workers_return_partial_reports_with_a_tool_free_final_round() {
        for limit in [Some(2), None] {
            let (host, ctx, server) = setup("budget-aware").await;
            let tool = tool(&host, ctx);
            let mut child = task("budget-aware");
            child.tools.push("scratchpad_note".into());
            child.max_tool_turns = limit;
            let expected = limit.unwrap_or(11);
            let started = host
                .subagent_command(&tool, Args::Spawn { task: child })
                .await
                .unwrap();
            let result = wait(&host, &tool, &started["run_id"]).await;
            let outcome = &result["result"]["outcome"];
            assert_eq!(outcome["status"], "partial");
            assert_eq!(outcome["answer"], "answer from budget-aware");
            assert_eq!(outcome["tool_rounds"], expected);
            assert_eq!(outcome["tool_round_limit"], expected);
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), expected + 1);
            assert!(requests[0]
                .to_string()
                .contains(&format!("remaining: {expected}/{expected}")));
            let final_request = requests.last().unwrap();
            assert!(
                final_request["tools"].is_null()
                    || final_request["tools"].as_array().is_some_and(Vec::is_empty)
            );
            assert!(final_request.to_string().contains("evidence-1"));
            assert!(final_request
                .to_string()
                .contains("single final reporting response"));
        }
    }

    #[tokio::test]
    async fn an_uncooperative_report_cannot_extend_the_model_loop() {
        let (host, ctx, server) = setup("budget-defiant").await;
        let tool = tool(&host, ctx);
        let mut child = task("budget-defiant");
        child.tools.push("scratchpad_note".into());
        child.max_tool_turns = Some(2);
        let started = host
            .subagent_command(&tool, Args::Spawn { task: child })
            .await
            .unwrap();
        let result = wait(&host, &tool, &started["run_id"]).await;
        assert_eq!(result["result"]["outcome"]["status"], "failed");
        assert!(result.to_string().contains("final report"));
        assert_eq!(server.requests.lock().unwrap().len(), 3);
    }
}
