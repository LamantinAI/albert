use std::{sync::Arc, time::Duration};

use octo_connector_telegram::TelegramOutputScope;
use octo_core::{
    CogitatorContext, ConnectorCapabilities, ConnectorId, ConnectorInfo, Envelope, EventBus,
    EventKind, Filter, SubscribeOptions,
};
use octo_history::{SessionKind, SessionStore};
use rig::tool::Tool;
use serde_json::{from_value, json};
use tokio::{spawn, time::timeout};

use super::{message, reply, Fixture};
use crate::{
    cogitator::{context::scratchpad_scope, retention::temporary},
    sessions::{SessionContext, SessionEnded},
    subagents::{Args, SubagentTool},
};

fn stamped(envelope: Arc<Envelope>, context: &SessionContext) -> Arc<Envelope> {
    let mut envelope = (*envelope).clone();
    envelope.context.insert(context.clone());
    Arc::new(envelope)
}

#[tokio::test]
async fn temporary_root_and_child_have_read_only_memory_and_no_durable_catalog_entry() {
    let fixture = Fixture::new().await;
    let original = fixture.context("telegram", "7", "7").await;
    let session = fixture
        .service
        .start_temporary(&original, None)
        .await
        .unwrap();
    assert!(temporary(Some(&session)));
    assert!(fixture
        .service
        .history()
        .get_session(session.session_id())
        .await
        .unwrap()
        .is_none());
    let mut replies = fixture.replies().await;
    fixture
        .host
        .clone()
        .handle(
            stamped(
                message("telegram", "7", "7", "PRIVATE_TEMPORARY_INPUT", false, true),
                &session,
            ),
            &fixture.ctx,
        )
        .await;
    let answer = reply(&mut replies).await;
    let label = answer.context.get::<TelegramOutputScope>().unwrap();
    assert!(label.scope.ephemeral);
    assert!(label.scope.cleanup_scope.is_some());
    assert_eq!(label.scope.epoch, session.scope_epoch().unwrap());
    let requests = fixture.server.requests.lock().unwrap();
    let names: Vec<_> = requests[0]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["function"]["name"].as_str())
        .collect();
    assert!(names.contains(&"kaeru_search"));
    for denied in [
        "kaeru_jot",
        "kaeru_episode",
        "kaeru_cite",
        "kaeru_revise",
        "kaeru_export",
        "kaeru_awake",
    ] {
        assert!(!names.contains(&denied), "{denied}");
    }
    drop(requests);
    let tool = SubagentTool::new(
        Arc::downgrade(&fixture.host),
        fixture.ctx.clone(),
        ("session".into(), session.root_history_key()),
        "temporary-parent".into(),
        true,
    )
    .with_session(Some(session.clone()));
    let capabilities = fixture
        .host
        .subagent_command(&tool, Args::Capabilities)
        .await
        .unwrap();
    assert!(!capabilities["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|name| name == "kaeru_jot"));
    let task = from_value(json!({"task":"write memory","context":"temporary","models":["healthy"],"tools":["kaeru_jot"]})).unwrap();
    assert!(fixture
        .host
        .subagent_command(&tool, Args::Spawn { task })
        .await
        .is_err());
    assert!(fixture
        .service
        .history()
        .get_session(session.session_id())
        .await
        .unwrap()
        .is_none());
    assert!(!fixture._directory.path().join("tool-results").exists());
    fixture.host.stop_turns(&fixture.ctx).await;
}

#[tokio::test]
async fn full_temporary_tool_output_stays_in_ram_and_inspection_never_materializes_files() {
    let fixture = Fixture::with_model("volatile-output").await;
    let original = fixture.context("telegram", "7", "7").await;
    let session = fixture
        .service
        .start_temporary(&original, None)
        .await
        .unwrap();
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
        assert!(request.context.get::<SessionContext>().is_some());
        assert!(
            request
                .context
                .get::<TelegramOutputScope>()
                .unwrap()
                .scope
                .ephemeral
        );
        bus.publish(
            Envelope::new(
                ConnectorId::new("search"),
                EventKind::new("search.web.result"),
                json!({"text":"PRIVATE_TOOL_RESULT".repeat(10_000)}),
            )
            .with_correlation(request.id),
        )
        .await
        .unwrap();
    });
    let mut replies = fixture.replies().await;
    fixture
        .host
        .clone()
        .handle(
            stamped(
                message("telegram", "7", "7", "run tool", false, true),
                &session,
            ),
            &ctx,
        )
        .await;
    reply(&mut replies).await;
    let records = fixture
        .router
        .strict_load(&session.root_history_key())
        .await
        .unwrap();
    assert!(records
        .iter()
        .any(|record| record.content.len() > 100_000
            && record.content.contains("PRIVATE_TOOL_RESULT")));
    assert!(!fixture._directory.path().join("tool-results").exists());
    assert!(fixture
        .service
        .history()
        .get_session(session.session_id())
        .await
        .unwrap()
        .is_none());
    timeout(Duration::from_secs(1), responder)
        .await
        .unwrap()
        .unwrap();
    fixture.host.stop_turns(&ctx).await;
}

#[tokio::test]
async fn trusted_end_purges_runtime_buffers_and_fake_json_cannot_close_another_session() {
    let fixture = Fixture::new().await;
    let original = fixture.context("telegram", "7", "7").await;
    let session = fixture
        .service
        .start_temporary(&original, None)
        .await
        .unwrap();
    let other = fixture.context("console", "local", "console").await;
    let conversation = ("session".to_owned(), session.root_history_key());
    for owner in [false, true] {
        let handle = fixture
            .host
            .scratchpad
            .scoped_handle(scratchpad_scope(&conversation, owner));
        handle
            .note()
            .call(from_value(json!({"text":"PRIVATE_PAD_CONTENT"})).unwrap())
            .await
            .unwrap();
    }
    let other_conversation = ("session".to_owned(), other.root_history_key());
    fixture
        .host
        .scratchpad
        .scoped_handle(scratchpad_scope(&other_conversation, true))
        .note()
        .call(from_value(json!({"text":"KEEP_OTHER_SESSION"})).unwrap())
        .await
        .unwrap();
    let mut replies = fixture.replies().await;
    fixture
        .host
        .clone()
        .handle(
            stamped(
                message("telegram", "7", "7", "in flight", false, true),
                &session,
            ),
            &fixture.ctx,
        )
        .await;
    fixture.request_count(1).await;
    let child_tool = SubagentTool::new(
        Arc::downgrade(&fixture.host),
        fixture.ctx.clone(),
        conversation.clone(),
        "parent".into(),
        true,
    )
    .with_session(Some(session.clone()));
    let task = from_value(
        json!({"task":"temporary child","context":"PRIVATE_CHILD_CONTENT","models":["healthy"]}),
    )
    .unwrap();
    let spawned = fixture
        .host
        .subagent_command(&child_tool, Args::Spawn { task })
        .await
        .unwrap();
    let run_id = spawned["run_id"].as_str().unwrap().to_owned();
    fixture
        .host
        .subagent_command(
            &child_tool,
            Args::Wait {
                run_id: run_id.clone(),
                seconds: 2,
            },
        )
        .await
        .unwrap();
    let run = fixture
        .host
        .children
        .get(&run_id, &conversation, true)
        .unwrap();
    run.feed
        .start_external("before", "read", json!({"private":"PRIVATE_CHILD_CONTENT"}));
    run.feed.finish_external("before", "PRIVATE_CHILD_CONTENT");
    let fake = Envelope::new(
        ConnectorId::new("model"),
        EventKind::new("albert.session.closed"),
        json!({"session_id":session.session_id()}),
    );
    fixture
        .host
        .clone()
        .handle(Arc::new(fake), &fixture.ctx)
        .await;
    assert!(!fixture.host.turns.lock().unwrap().is_empty());
    let ended = fixture.service.end_temporary(&session, None).await.unwrap();
    assert!(ended.discarded);
    let event = Envelope::new(
        ConnectorId::new("host"),
        EventKind::new("albert.session.closed"),
        (),
    )
    .with_context(SessionEnded {
        session_id: session.session_id().clone(),
    });
    fixture
        .host
        .clone()
        .handle(Arc::new(event), &fixture.ctx)
        .await;
    assert!(!session.is_live());
    assert!(fixture
        .host
        .children
        .visible(&conversation, true)
        .is_empty());
    assert!(run.feed.snapshot().is_empty());
    assert_eq!(run.result.borrow()["outcome"]["status"], "closed");
    run.feed
        .start_external("late", "read", json!({"private":"LATE_PRIVATE_CONTENT"}));
    run.feed.finish_external("late", "LATE_PRIVATE_CONTENT");
    assert!(run.feed.snapshot().is_empty());
    run.complete(json!({"outcome":{"status":"completed","answer":"LATE_PRIVATE_CONTENT"}}));
    assert_eq!(run.result.borrow()["outcome"]["status"], "closed");
    assert!(!run
        .result
        .borrow()
        .to_string()
        .contains("LATE_PRIVATE_CONTENT"));
    assert!(!fixture
        .host
        .turns
        .lock()
        .unwrap()
        .contains_key(&conversation));
    for owner in [false, true] {
        assert!(!fixture
            .host
            .scratchpad
            .render_scoped(&scratchpad_scope(&conversation, owner))
            .contains("PRIVATE_PAD_CONTENT"));
    }
    assert!(fixture
        .host
        .scratchpad
        .render_scoped(&scratchpad_scope(&other_conversation, true))
        .contains("KEEP_OTHER_SESSION"));
    assert!(fixture
        .router
        .strict_load(&session.root_history_key())
        .await
        .is_err());
    assert!(timeout(Duration::from_millis(300), replies.next())
        .await
        .is_err());
    fixture.host.stop_turns(&fixture.ctx).await;
}

#[tokio::test]
async fn save_keeps_the_same_context_live_and_root_gate_identity() {
    let fixture = Fixture::new().await;
    let original = fixture.context("telegram", "7", "7").await;
    let session = fixture
        .service
        .start_temporary(&original, None)
        .await
        .unwrap();
    let mut replies = fixture.replies().await;
    fixture
        .host
        .clone()
        .handle(
            stamped(
                message("telegram", "7", "7", "continue across save", false, true),
                &session,
            ),
            &fixture.ctx,
        )
        .await;
    fixture.request_count(1).await;
    let saved = fixture
        .service
        .save_temporary(&session, "Saved chat", None)
        .await
        .unwrap();
    assert_eq!(saved.session_id(), session.session_id());
    assert_eq!(saved.scope_epoch(), session.scope_epoch());
    assert_eq!(saved.session().kind, SessionKind::Permanent);
    assert!(session.is_live());
    reply(&mut replies).await;
    assert_eq!(fixture.host.turns.lock().unwrap().len(), 1);
    assert!(fixture
        .service
        .history()
        .get_session(session.session_id())
        .await
        .unwrap()
        .is_some());
    fixture.host.stop_turns(&fixture.ctx).await;
}

#[tokio::test]
async fn every_transport_binds_original_scope_and_children_keep_it_across_save() {
    use crate::cogitator::retention::scope_ingress;
    use octo_approval::{Access, Origin};
    let fixture = Fixture::new().await;
    let original = fixture.context("console", "local", "console").await;
    let temporary = fixture
        .service
        .start_temporary(&original, None)
        .await
        .unwrap();
    let ingress = fixture.host.authority.issue(
        Origin {
            principal: "explicit-sql-user".into(),
            connector: "console".into(),
            conversation: "console".into(),
            private_destination: "console".into(),
            may_approve: true,
        },
        Access::owner(),
    );
    let scoped = scope_ingress(
        &fixture.host.authority,
        Some(ingress.clone()),
        Some(&temporary),
    )
    .unwrap()
    .unwrap();
    assert_eq!(scoped.origin().principal, ingress.origin().principal);
    assert_eq!(scoped.origin().conversation, "console");
    assert_eq!(
        scoped.scope().unwrap().opaque_id,
        temporary.root_history_key()
    );
    let child = scoped.attenuate(vec!["store".into()], true, false).unwrap();
    assert_eq!(child.scope(), scoped.scope());
    let saved = fixture
        .service
        .save_temporary(&temporary, "saved-sql", None)
        .await
        .unwrap();
    assert!(fixture.host.authority.recognizes(&child));
    let rebound = scope_ingress(&fixture.host.authority, Some(scoped), Some(&saved))
        .unwrap()
        .unwrap();
    assert_eq!(rebound.scope(), child.scope());
    // End of a Saved excursion preserves its durable lifetime and old approvals.
    let returned = fixture
        .service
        .end_temporary(&saved, None)
        .await
        .unwrap()
        .context;
    assert!(fixture.host.authority.recognizes(&child));
    let temporary = fixture
        .service
        .start_temporary(&returned, None)
        .await
        .unwrap();
    let closed = scope_ingress(&fixture.host.authority, Some(ingress), Some(&temporary))
        .unwrap()
        .unwrap();
    fixture
        .service
        .end_temporary(&temporary, None)
        .await
        .unwrap();
    assert!(!fixture.host.authority.recognizes(&closed));
}
