use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};

use sqlx::query;
use tokio::{
    spawn,
    time::{sleep, timeout},
};

use super::{message, reply, setup, Fixture};
use crate::{cogitator::UserInput, history::sessions::Backend, sessions::SessionContext};

#[tokio::test]
async fn replay_after_selector_change_and_fresh_host_uses_original_receipt() {
    let fixture = Fixture::new().await;
    let mut replies = fixture.replies().await;
    let first = message(
        "telegram",
        "7",
        "7",
        "durable original request",
        false,
        true,
    );
    fixture
        .host
        .clone()
        .handle(first.clone(), &fixture.ctx)
        .await;
    reply(&mut replies).await;
    let original = fixture.context("telegram", "7", "7").await;
    let selected = fixture
        .service
        .create_private(&original, "New selector")
        .await
        .unwrap();
    // A fresh cogitator has no in-memory request locks or request cache. Replay
    // must find the retained original receipt before looking at this selector.
    let (host, _, server) = setup("session-delayed").await;
    let host = host.with_sessions(fixture.service.clone());
    host.clone().handle(first, &fixture.ctx).await;
    assert_eq!(host.turn_seq.load(Ordering::Relaxed), 0);
    assert!(server.requests.lock().unwrap().is_empty());
    assert!(fixture
        .router
        .strict_load(&selected.root_history_key())
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        fixture
            .router
            .strict_load(&original.root_history_key())
            .await
            .unwrap()
            .len(),
        2
    );
    host.stop_turns(&fixture.ctx).await;
}

#[tokio::test]
async fn changed_body_conflicts_without_interrupting_original_work() {
    let fixture = Fixture::new().await;
    let mut replies = fixture.replies().await;
    let first = message("telegram", "7", "7", "accepted body", false, true);
    fixture
        .host
        .clone()
        .handle(first.clone(), &fixture.ctx)
        .await;
    fixture.request_count(1).await;
    let mut changed = (*message("telegram", "7", "7", "changed body", false, true)).clone();
    changed.id = first.id;
    fixture
        .host
        .clone()
        .handle(Arc::new(changed), &fixture.ctx)
        .await;
    let rejected = reply(&mut replies).await;
    assert!(rejected
        .payload_as::<String>()
        .unwrap()
        .contains("not accepted"));
    assert_eq!(fixture.host.turn_seq.load(Ordering::Relaxed), 1);
    let completed = reply(&mut replies).await;
    assert_eq!(
        completed.payload_as::<String>().unwrap(),
        "answer from session-delayed"
    );
    fixture.host.stop_turns(&fixture.ctx).await;
}

#[tokio::test]
async fn final_persistence_failure_does_not_publish_the_provider_success() {
    let fixture = Fixture::new().await;
    let mut replies = fixture.replies().await;
    fixture
        .host
        .clone()
        .handle(
            message("telegram", "7", "7", "work", false, true),
            &fixture.ctx,
        )
        .await;
    fixture.request_count(1).await;
    query("DROP TABLE journal_append_records")
        .execute(fixture.service.history().pool())
        .await
        .unwrap();
    let failed = reply(&mut replies).await;
    let text = failed.payload_as::<String>().unwrap();
    assert!(text.contains("couldn't record"));
    assert!(!text.contains("answer from session-delayed"));
    fixture.host.stop_turns(&fixture.ctx).await;
}

#[tokio::test]
async fn concurrent_native_copy_waits_for_original_binding_across_selector_change() {
    let fixture = Fixture::new().await;
    let mut replies = fixture.replies().await;
    let first = message(
        "telegram",
        "7",
        "7",
        "original concurrent request",
        false,
        true,
    );
    let admitted = fixture
        .host
        .admit_message(first.clone(), &fixture.ctx)
        .await
        .unwrap();
    let original = admitted.context.get::<SessionContext>().unwrap().clone();
    let host = fixture.host.clone();
    let ctx = fixture.ctx.clone();
    let duplicate = spawn(async move {
        host.handle(first, &ctx).await;
    });
    timeout(Duration::from_secs(2), async {
        loop {
            if fixture
                .host
                .request_gates
                .lock()
                .unwrap()
                .values()
                .any(|gate| gate.strong_count() >= 2)
            {
                break;
            }
            sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    // Navigation now takes the lifecycle writer: it waits for the already
    // admitted input to append, while the concurrent duplicate stays serialized.
    let service = fixture.service.clone();
    let navigation_context = original.clone();
    let navigation = spawn(async move {
        service
            .create_private(&navigation_context, "Changed after original append")
            .await
    });
    sleep(Duration::from_millis(10)).await;
    assert!(!navigation.is_finished());
    fixture
        .host
        .clone()
        .spawn_turn(
            admitted,
            UserInput {
                text: "original concurrent request".into(),
                images: Vec::new(),
                seed: None,
                voice: None,
            },
            &fixture.ctx,
        )
        .await;
    let selected = timeout(Duration::from_secs(2), navigation)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    timeout(Duration::from_secs(2), duplicate)
        .await
        .unwrap()
        .unwrap();
    reply(&mut replies).await;
    assert_eq!(fixture.host.turn_seq.load(Ordering::Relaxed), 1);
    assert!(fixture
        .router
        .strict_load(&selected.root_history_key())
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        fixture
            .router
            .strict_load(&original.root_history_key())
            .await
            .unwrap()
            .len(),
        2
    );
    fixture.host.stop_turns(&fixture.ctx).await;
}

#[tokio::test]
async fn closed_backend_suppresses_late_terminal_delivery() {
    let fixture = Fixture::new().await;
    let mut replies = fixture.replies().await;
    fixture
        .host
        .clone()
        .handle(
            message("telegram", "7", "7", "in flight while closed", false, true),
            &fixture.ctx,
        )
        .await;
    fixture.request_count(1).await;
    let context = fixture.context("telegram", "7", "7").await;
    *fixture
        .router
        .handle(context.session_id())
        .await
        .write()
        .await = Backend::Closed;
    assert!(timeout(Duration::from_millis(350), replies.next())
        .await
        .is_err());
    fixture.host.stop_turns(&fixture.ctx).await;
}
