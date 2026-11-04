use std::{sync::atomic::Ordering, sync::Arc};

use octo_core::{ConnectorId, Envelope, EventKind};
use octo_rig::DispatchArgs;
use serde_json::json;

use super::{
    provider_fixture::setup,
    session_dispatch::pin_alarm,
    testing::sessions::{message, reply, Fixture},
};

#[tokio::test]
async fn linked_transports_share_gate_and_retry_does_not_interrupt_or_append() {
    let fixture = Fixture::new().await;
    let mut replies = fixture.replies().await;
    let first = message("telegram", "7", "7", "first private input", false, true);
    fixture
        .host
        .clone()
        .handle(first.clone(), &fixture.ctx)
        .await;
    fixture.request_count(1).await;
    let original = fixture.context("telegram", "7", "7").await;
    let console = fixture.context("console", "local", "console").await;
    fixture
        .service
        .open_private(
            &console,
            original.session_id(),
            console.selector_revision().unwrap(),
        )
        .await
        .unwrap();
    let seq = fixture.host.turn_seq.load(Ordering::Relaxed);
    fixture.host.clone().handle(first, &fixture.ctx).await;
    assert_eq!(fixture.host.turn_seq.load(Ordering::Relaxed), seq);
    let second = message(
        "console",
        "local",
        "console",
        "second private input",
        false,
        true,
    );
    fixture
        .host
        .clone()
        .handle(second.clone(), &fixture.ctx)
        .await;
    fixture.request_count(2).await;
    let answer = reply(&mut replies).await;
    assert_eq!(answer.correlation_id, Some(second.id));
    assert_eq!(answer.target.as_ref().unwrap().as_str(), "console");
    assert_eq!(answer.channel.as_ref().unwrap().as_str(), "console");
    assert_eq!(fixture.host.turns.lock().unwrap().len(), 1);
    let stored = fixture
        .router
        .strict_load(&original.root_history_key())
        .await
        .unwrap();
    assert_eq!(
        stored
            .iter()
            .filter(|turn| turn.content.contains("first private input"))
            .count(),
        1
    );
    assert_eq!(
        stored
            .iter()
            .filter(|turn| turn.content.contains("second private input"))
            .count(),
        1
    );
    let requests = fixture.server.requests.lock().unwrap();
    assert!(requests[1]["messages"]
        .to_string()
        .contains("first private input"));
    drop(requests);
    fixture.host.stop_turns(&fixture.ctx).await;
}

#[tokio::test]
async fn different_selected_sessions_keep_late_replies_and_contexts_separate() {
    let fixture = Fixture::new().await;
    let mut replies = fixture.replies().await;
    let first = message("telegram", "7", "7", "first-session-secret", false, true);
    fixture
        .host
        .clone()
        .handle(first.clone(), &fixture.ctx)
        .await;
    fixture.request_count(1).await;
    let original = fixture.context("telegram", "7", "7").await;
    let selected = fixture
        .service
        .create_private(&original, "Second")
        .await
        .unwrap();
    let second = message("telegram", "7", "7", "second-session-question", false, true);
    fixture
        .host
        .clone()
        .handle(second.clone(), &fixture.ctx)
        .await;
    fixture.request_count(2).await;
    let a = reply(&mut replies).await;
    let b = reply(&mut replies).await;
    assert!(a.correlation_id == Some(first.id) || b.correlation_id == Some(first.id));
    assert!(a.correlation_id == Some(second.id) || b.correlation_id == Some(second.id));
    assert_eq!(fixture.host.turns.lock().unwrap().len(), 2);
    let requests = fixture.server.requests.lock().unwrap();
    assert!(!requests[1]["messages"]
        .to_string()
        .contains("first-session-secret"));
    drop(requests);
    assert_eq!(
        fixture
            .router
            .strict_load(&original.root_history_key())
            .await
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        fixture
            .router
            .strict_load(&selected.root_history_key())
            .await
            .unwrap()
            .len(),
        2
    );
    fixture.host.stop_turns(&fixture.ctx).await;
}

#[tokio::test]
async fn group_chatter_is_quiet_and_owners_do_not_transfer_authority_to_other_actors() {
    let fixture = Fixture::new().await;
    let mut replies = fixture.replies().await;
    let first = message("telegram", "7", "group", "owner group task", true, true);
    fixture
        .host
        .clone()
        .handle(first.clone(), &fixture.ctx)
        .await;
    fixture.request_count(1).await;
    let seq = fixture.host.turn_seq.load(Ordering::Relaxed);
    let mut quiet = (*message("telegram", "8", "group", "quiet chatter", true, false)).clone();
    quiet
        .channel_metadata
        .as_mut()
        .unwrap()
        .tags
        .insert("addressed".into(), "false".into());
    fixture
        .host
        .clone()
        .handle(Arc::new(quiet), &fixture.ctx)
        .await;
    assert_eq!(fixture.host.turn_seq.load(Ordering::Relaxed), seq);
    let next = message("telegram", "8", "group", "ordinary group task", true, false);
    fixture
        .host
        .clone()
        .handle(next.clone(), &fixture.ctx)
        .await;
    fixture.request_count(2).await;
    let answer = reply(&mut replies).await;
    assert_eq!(answer.correlation_id, Some(next.id));
    let wire = fixture.server.requests.lock().unwrap()[1]["tools"].to_string();
    assert!(!wire.contains("config_set_secret"));
    assert!(!wire.contains("restart"));
    assert_eq!(fixture.host.turns.lock().unwrap().len(), 1);
    fixture.host.stop_turns(&fixture.ctx).await;
}

#[tokio::test]
async fn scheduler_payload_is_host_pinned_and_legacy_alarm_uses_original_default() {
    let fixture = Fixture::new().await;
    let original = fixture.context("telegram", "7", "7").await;
    let mut args = DispatchArgs {
        target: "scheduler".into(),
        kind: "octo.scheduler.add_alarm".into(),
        channel: None,
        payload: json!({"payload":{"task":"check experiment","channel":"attacker","reply_via":"other","albert_session_id":"forged","routine":"memory_reflection"}}),
    };
    pin_alarm(&mut args, Some(&original)).unwrap();
    assert_eq!(
        args.payload["payload"]["albert_session_id"],
        original.session_id().as_str()
    );
    assert_eq!(args.payload["payload"]["channel"], "7");
    assert!(args.payload["payload"].get("routine").is_none());
    let selected = fixture
        .service
        .create_private(&original, "Other work")
        .await
        .unwrap();
    let mut replies = fixture.replies().await;
    let fired = Arc::new(
        Envelope::new(
            ConnectorId::new("scheduler"),
            EventKind::new("alarm.fired"),
            json!({"task":"original task","channel":"7","reply_via":"telegram"}),
        )
        .with_tag("alarm_id", "legacy-alarm"),
    );
    fixture.host.on_alarm(fired.clone(), &fixture.ctx).await;
    let answer = reply(&mut replies).await;
    assert_eq!(answer.channel.as_ref().unwrap().as_str(), "7");
    assert_eq!(answer.target.as_ref().unwrap().as_str(), "telegram");
    assert_eq!(answer.correlation_id, Some(fired.id));
    assert_eq!(
        fixture
            .router
            .strict_load(&original.root_history_key())
            .await
            .unwrap()
            .len(),
        2
    );
    assert!(fixture
        .router
        .strict_load(&selected.root_history_key())
        .await
        .unwrap()
        .is_empty());
    fixture.host.stop_turns(&fixture.ctx).await;
}

mod child;
mod compaction;
mod feedback;
mod migration;
mod navigation;
mod reattach;
mod replay;
mod temporary;
mod titles;

mod web;
