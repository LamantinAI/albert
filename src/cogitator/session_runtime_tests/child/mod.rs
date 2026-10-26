use std::{sync::Arc, time::Duration};

use octo_core::{
    CogitatorContext, ConnectorCapabilities, ConnectorId, ConnectorInfo, Envelope, EventBus,
    EventKind, Filter, SubscribeOptions,
};
use serde_json::{from_value, json};
use tokio::{spawn, time::timeout};

use super::Fixture;
use crate::subagents::{Args, SubagentTool};

#[tokio::test]
async fn child_uses_typed_session_journal_and_keeps_physical_dispatch_origin() {
    let fixture = Fixture::with_model("child-dispatch").await;
    let session = fixture.context("telegram", "7", "7").await;
    let mut connectors = fixture.ctx.connectors().to_vec();
    connectors.push(ConnectorInfo {
        id: ConnectorId::new("search"),
        capabilities: ConnectorCapabilities {
            description: Some("search.web {query}".into()),
            event_kinds_accept: vec![EventKind::new("search.web")],
            ..Default::default()
        },
    });
    let ctx = CogitatorContext::new(fixture.ctx.shutdown.clone(), fixture.ctx.bus(), connectors);
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
        assert_eq!(request.tags["origin.connector"], "telegram");
        assert_eq!(request.tags["origin.channel"], "7");
        assert!(request.channel.is_none());
        bus.publish(
            Envelope::new(
                ConnectorId::new("search"),
                EventKind::new("search.web.result"),
                json!({"result":"verified"}),
            )
            .with_correlation(request.id),
        )
        .await
        .unwrap();
    });
    let tool = SubagentTool::new(
        Arc::downgrade(&fixture.host),
        ctx,
        ("session".into(), session.root_history_key()),
        "parent-test".into(),
        true,
    )
    .with_session(Some(session.clone()));
    let task = from_value(json!({"task":"Look up one fact","context":"explicit child input","models":["child-dispatch"],"connectors":["search"],"tools":[]})).unwrap();
    let spawned = fixture
        .host
        .subagent_command(&tool, Args::Spawn { task })
        .await
        .unwrap();
    let run_id = spawned["run_id"].as_str().unwrap().to_owned();
    let result = timeout(
        Duration::from_secs(4),
        fixture.host.subagent_command(
            &tool,
            Args::Wait {
                run_id: run_id.clone(),
                seconds: 3,
            },
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(result["result"]["journal_saved"], true);
    let key = format!("{}/child/{run_id}", session.root_history_key());
    assert!(fixture.router.strict_load(&key).await.unwrap().len() >= 2);
    assert!(fixture
        .router
        .strict_load(&session.root_history_key())
        .await
        .unwrap()
        .is_empty());
    timeout(Duration::from_secs(1), responder)
        .await
        .unwrap()
        .unwrap();
    fixture.host.stop_turns(&fixture.ctx).await;
}
