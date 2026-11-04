use std::{sync::Arc, time::Duration};

use albert_connector_telegram::TelegramTransport;
use albert_core::{
    cogitator::testing::message,
    cogitator::testing::sessions::reply,
    cogitator::testing::SessionFixture as Fixture,
    cogitator::testing::{admit, attempts, configure},
    sessions::transport::OutputScope as TelegramOutputScope,
    sessions::transport::{HostTransport, NativeIngress},
    sessions::SessionContext,
};
use octo_connector_telegram::{NativeBatch, NativeFrame};
use octo_core::{ConnectorId, Envelope};
use tokio::time::timeout;

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
    let mut fixture = Fixture::new().await;
    configure(&mut fixture.host, Arc::new(TelegramTransport), |_| {});
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
        .panel_handle(first.clone(), &fixture.ctx)
        .await;
    let answer = reply(&mut replies).await;
    let scope = answer.context.get::<TelegramOutputScope>().unwrap();
    assert_eq!(scope.scope.key, original.root_history_key());
    assert!(answer.context.get::<SessionContext>().unwrap().is_live());
    let mapped = TelegramTransport.ingress(first.clone());
    for frame in &mapped.context.get::<NativeIngress>().unwrap().constituents {
        let alias = frame.alias.clone();
        let bound = fixture
            .router
            .find_request(&alias.namespace, &alias.idempotency_key)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(bound.digest, frame.alias.digest);
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
    fixture.host.clone().panel_handle(retry, &fixture.ctx).await;
    assert_eq!(attempts(&fixture.host), 1);
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
    fixture.host.panel_stop(&fixture.ctx).await;
}

#[tokio::test]
async fn partial_native_replay_and_changed_constituent_fail_before_interrupt() {
    let mut fixture = Fixture::new().await;
    configure(&mut fixture.host, Arc::new(TelegramTransport), |_| {});
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
        .panel_handle(first.clone(), &fixture.ctx)
        .await;
    fixture.request_count(1).await;
    let mixed = native(
        message("telegram", "7", "7", "overlap", false, true),
        Some(original.clone()),
        &[2, 3],
    );
    assert!(admit(&fixture.host, mixed, &fixture.ctx).await.is_err());
    let mut changed = (*first).clone();
    let mut frames = changed.context.get::<NativeBatch>().unwrap().frames.clone();
    frames[0].digest = "changed-native-body".into();
    changed.context.insert(NativeBatch { frames });
    assert!(admit(&fixture.host, Arc::new(changed), &fixture.ctx)
        .await
        .is_err());
    assert_eq!(attempts(&fixture.host), 1);
    assert_eq!(
        reply(&mut replies).await.payload_as::<String>().unwrap(),
        "answer from session-delayed"
    );
    fixture.host.panel_stop(&fixture.ctx).await;
}

#[tokio::test]
async fn trusted_stamp_is_kept_when_selector_changes_during_io() {
    let mut fixture = Fixture::new().await;
    configure(&mut fixture.host, Arc::new(TelegramTransport), |_| {});
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
    fixture
        .host
        .clone()
        .panel_handle(stamped, &fixture.ctx)
        .await;
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
    fixture.host.panel_stop(&fixture.ctx).await;
}
