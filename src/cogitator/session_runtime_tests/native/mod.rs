use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};

use octo_connector_telegram::{NativeBatch, NativeFrame, TelegramOutputScope};
use octo_core::{ConnectorId, Envelope};
use tokio::time::timeout;

use super::{message, reply, Fixture};
use crate::sessions::{native::request_alias, SessionContext};

fn native(base: Arc<Envelope>, session: Option<SessionContext>, ids: &[u32]) -> Arc<Envelope> {
    let mut envelope = (*base).clone();
    envelope.context.insert(NativeBatch {
        frames: ids
            .iter()
            .map(|id| NativeFrame {
                bot_id: 100,
                connector: ConnectorId::new("telegram"),
                update_id: *id,
                message_id: *id as i32,
                date: 1_000_000 + i64::from(*id),
                chat_id: 7,
                actor_id: Some(7),
                chat_type: "private".into(),
                command_text: None,
                digest: format!("native-content-{id}"),
                media: false,
            })
            .collect(),
    });
    if let Some(session) = session {
        envelope.context.insert(session);
    }
    Arc::new(envelope)
}

#[tokio::test]
async fn native_batch_retains_every_alias_and_replays_after_selector_change() {
    let fixture = Fixture::new().await;
    let original = fixture.context("telegram", "7", "7").await;
    let first = native(
        message("telegram", "7", "7", "first\nsecond", false, true),
        Some(original.clone()),
        &[1, 2],
    );
    let mut replies = fixture.replies().await;
    fixture
        .host
        .clone()
        .handle(first.clone(), &fixture.ctx)
        .await;
    let answer = reply(&mut replies).await;
    let scope = answer.context.get::<TelegramOutputScope>().unwrap();
    assert_eq!(scope.scope.key, original.root_history_key());
    assert!(answer.context.get::<SessionContext>().unwrap().is_live());
    for frame in &first.context.get::<NativeBatch>().unwrap().frames {
        let alias = request_alias(frame);
        let bound = fixture
            .router
            .find_request(&alias.namespace, &alias.idempotency_key)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(bound.digest, frame.digest);
        assert_eq!(bound.journal.session_id(), original.session_id());
    }
    let selected = fixture
        .service
        .create_private(&original, "Other session")
        .await
        .unwrap();
    let retry = native(
        message(
            "telegram",
            "7",
            "7",
            "same native frames after enrichment",
            false,
            true,
        ),
        Some(original.clone()),
        &[1, 2],
    );
    fixture.host.clone().handle(retry, &fixture.ctx).await;
    assert_eq!(fixture.host.turn_seq.load(Ordering::Relaxed), 1);
    assert!(fixture
        .router
        .strict_load(&selected.root_history_key())
        .await
        .unwrap()
        .is_empty());
    let records = fixture
        .router
        .strict_load(&original.root_history_key())
        .await
        .unwrap();
    assert_eq!(records.len(), 2);
    assert!(records[0].content.contains("native_frames"));
    assert!(records[0].content.contains("\"message_id\":1"));
    assert!(records[0].content.contains("\"message_id\":2"));
    fixture.host.stop_turns(&fixture.ctx).await;
}

#[tokio::test]
async fn partial_native_replay_and_changed_constituent_fail_before_interrupt() {
    let fixture = Fixture::new().await;
    let original = fixture.context("telegram", "7", "7").await;
    let first = native(
        message("telegram", "7", "7", "accepted batch", false, true),
        Some(original.clone()),
        &[1, 2],
    );
    let mut replies = fixture.replies().await;
    fixture
        .host
        .clone()
        .handle(first.clone(), &fixture.ctx)
        .await;
    fixture.request_count(1).await;
    let mixed = native(
        message("telegram", "7", "7", "overlap", false, true),
        Some(original.clone()),
        &[2, 3],
    );
    assert!(fixture
        .host
        .admit_message(mixed, &fixture.ctx)
        .await
        .is_err());
    let mut changed = (*first).clone();
    let mut frames = changed.context.get::<NativeBatch>().unwrap().frames.clone();
    frames[0].digest = "changed-native-body".into();
    changed.context.insert(NativeBatch { frames });
    assert!(fixture
        .host
        .admit_message(Arc::new(changed), &fixture.ctx)
        .await
        .is_err());
    assert_eq!(fixture.host.turn_seq.load(Ordering::Relaxed), 1);
    assert_eq!(
        reply(&mut replies).await.payload_as::<String>().unwrap(),
        "answer from session-delayed"
    );
    fixture.host.stop_turns(&fixture.ctx).await;
}

#[tokio::test]
async fn trusted_stamp_is_kept_when_selector_changes_during_io() {
    let fixture = Fixture::new().await;
    let original = fixture.context("telegram", "7", "7").await;
    let stamped = native(
        message("telegram", "7", "7", "pinned before IO", false, true),
        Some(original.clone()),
        &[8],
    );
    let selected = fixture
        .service
        .create_private(&original, "New navigation target")
        .await
        .unwrap();
    let mut replies = fixture.replies().await;
    fixture.host.clone().handle(stamped, &fixture.ctx).await;
    reply(&mut replies).await;
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
    assert!(timeout(Duration::from_millis(20), replies.next())
        .await
        .is_err());
    fixture.host.stop_turns(&fixture.ctx).await;
}
