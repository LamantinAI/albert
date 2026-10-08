//! End-to-end checks at the model/tool boundary, for root and delegated loops.
use std::{sync::Arc, time::Duration};

use octo_core::{
    CogitatorContext, ConnectorCapabilities, ConnectorId, ConnectorInfo, Envelope, EventBus,
    EventKind, Filter, SubscribeOptions,
};
use rig::completion::Message;
use serde_json::json;
use tempfile::tempdir;
use tokio::{spawn, time::timeout};

use super::provider_fixture::setup;
use crate::{
    status::StatusFeed,
    subagents::{Args, SubagentTool},
};

#[tokio::test]
async fn root_and_child_only_send_bounded_artifact_evidence_to_the_provider() {
    for child in [false, true] {
        let (mut host, ctx, server) = setup("large-output").await;
        let dir = tempdir().unwrap();
        Arc::get_mut(&mut host).unwrap().config.code_workspace = dir.path().into();
        let ctx = CogitatorContext::new(
            ctx.shutdown.clone(),
            ctx.bus(),
            vec![ConnectorInfo {
                id: ConnectorId::new("search"),
                capabilities: ConnectorCapabilities {
                    description: Some("search.web {query}".into()),
                    event_kinds_accept: vec![EventKind::new("search.web")],
                    ..Default::default()
                },
            }],
        );
        let mut requests = ctx
            .bus()
            .subscribe(Filter::by_kind("search.web"), SubscribeOptions::default())
            .await
            .unwrap();
        let bus = ctx.bus();
        let responder = spawn(async move {
            let request = timeout(Duration::from_secs(3), requests.next())
                .await
                .unwrap()
                .unwrap();
            let text = format!(
                "{}evidence-after-preview{}",
                "a".repeat(20000),
                "z".repeat(1000000)
            );
            bus.publish(
                Envelope::new(
                    ConnectorId::new("search"),
                    EventKind::new("search.result"),
                    json!({"text":text,"url":"https://example.test/source"}),
                )
                .with_target(request.source.clone())
                .with_correlation(request.id),
            )
            .await
            .unwrap();
        });
        if child {
            let tool = SubagentTool::new(
                Arc::downgrade(&host),
                ctx.clone(),
                ("telegram".into(), "room".into()),
                "root".into(),
                true,
            );
            let task = serde_json::from_value(
                json!({"task":"research","models":["large-output"],"connectors":["search"]}),
            )
            .unwrap();
            let started = host
                .subagent_command(&tool, Args::Spawn { task })
                .await
                .unwrap();
            let result = timeout(
                Duration::from_secs(5),
                host.subagent_command(
                    &tool,
                    Args::Wait {
                        run_id: started["run_id"].as_str().unwrap().into(),
                        seconds: 4,
                    },
                ),
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(result["result"]["outcome"]["status"], "completed");
        } else {
            let feed = StatusFeed::silent();
            let (answer, _) = timeout(
                Duration::from_secs(5),
                host.run_agent(
                    &ctx,
                    "room",
                    "test",
                    Message::user("research"),
                    vec![],
                    Some(ConnectorId::new("telegram")),
                    true,
                    feed.clone(),
                    Some("root"),
                ),
            )
            .await
            .unwrap();
            assert_eq!(answer, "answer from large-output");
            let journal = serde_json::to_string(&feed.snapshot()).unwrap();
            assert!(journal.len() < 30000 && journal.contains("artifact_id"));
        }
        responder.await.unwrap();
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        for r in requests.iter() {
            assert!(r["messages"].to_string().len() < 30000);
        }
        assert!(requests[2]["messages"]
            .to_string()
            .contains("evidence-after-preview"));
        drop(requests);
        host.stop_turns(&ctx).await;
    }
}
