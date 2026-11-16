use albert_connector_telegram::{SessionTelegramPolicy, TelegramTransport};
use albert_core::{
    cogitator::testing::{authority, configure, MessagingFixture as Fixture},
    messaging::contract::MessageId,
    messaging::store::DeliveryState,
    messaging::transport::ReportAttempt,
    sessions::presentation::OrdinarySessionOutput,
};
use octo_connector_telegram::{DeliveryReport, DeliveryStatus, TelegramHostPolicy};
use octo_history::SessionStore;
use serde_json::{from_value, json};
use std::{sync::Arc, time::Duration};
use tokio::{time::sleep, time::timeout};
#[allow(dead_code)]
#[path = "common/visibility.rs"]
mod fixture;
use self::fixture::{one, replies, response, return_active};

#[tokio::test]
async fn inactive_implicit_response_is_canonical_but_known_not_sent_and_never_replayed_on_open() {
    let mut fixture = Fixture::new("healthy").await;
    configure(&mut fixture.host, Arc::new(TelegramTransport), |_| {});
    let mut events = replies(&fixture).await;
    let id = response(&fixture, "background reply").await;
    fixture.host.panel_maintenance(&fixture.ctx).await;
    let status = fixture.status(&id).await;
    assert!(status.received.is_some());
    assert_eq!(status.delivery, DeliveryState::Failed);
    assert_eq!(status.failure_code.as_deref(), Some("inactive_view"));
    assert!(status.physical.submitted_at.is_none());
    assert!(status.physical.completion_note.contains("Not sent"));
    let _ = return_active(&fixture).await;
    for _ in 0..3 {
        fixture.host.panel_maintenance(&fixture.ctx).await;
    }
    assert!(timeout(Duration::from_millis(30), events.next())
        .await
        .is_err());
    assert!(fixture.server.requests.lock().unwrap().is_empty());
}
#[tokio::test]
async fn queued_transport_frame_rechecks_current_view_and_retains_partial_unknown_evidence() {
    let mut fixture = Fixture::new("healthy").await;
    configure(&mut fixture.host, Arc::new(TelegramTransport), |_| {});
    let active = return_active(&fixture).await;
    let mut events = replies(&fixture).await;
    let id = response(&fixture, "{\"ordinary\":false,\"report\":true}").await;
    fixture.host.panel_maintenance(&fixture.ctx).await;
    let frame = one(&mut events).await;
    assert!(frame.context.get::<OrdinarySessionOutput>().is_some());
    assert!(frame
        .payload_as::<String>()
        .unwrap()
        .contains(" · host\n\n{"));
    assert!(frame
        .payload_as::<String>()
        .unwrap()
        .starts_with("Reply from session"));
    let policy = SessionTelegramPolicy::new(fixture.service.clone(), authority(&fixture.host));
    policy.delivered(
        &frame,
        &DeliveryReport {
            scope: None,
            chat_id: 7,
            message_ids: vec![81],
            status: DeliveryStatus::Sent,
        },
    );
    timeout(Duration::from_secs(2), async {
        while fixture.status(&id).await.physical.observations.is_empty() {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let _ = fixture
        .service
        .open_private(
            &active,
            fixture.b.session_id(),
            active.selector_revision().unwrap(),
        )
        .await
        .unwrap();
    assert!(!policy.allow_delivery(&frame));
    timeout(Duration::from_secs(2), async {
        while fixture.status(&id).await.delivery != DeliveryState::Unknown {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    policy.delivered(
        &frame,
        &DeliveryReport {
            scope: None,
            chat_id: 7,
            message_ids: vec![82],
            status: DeliveryStatus::Sent,
        },
    );
    timeout(Duration::from_secs(2), async {
        while fixture.status(&id).await.physical.observations.len() < 3 {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let status = fixture.status(&id).await;
    assert_eq!(status.delivery, DeliveryState::Unknown);
    assert!(status
        .physical
        .observations
        .iter()
        .any(|entry| entry.outcome == "view_suppressed"));
    assert!(!status.physical.completion_note.contains("Not sent"));
}
#[tokio::test]
async fn equal_explicit_final_keeps_canonical_response_and_one_visible_notice_for_success_and_failure(
) {
    for cancel in [false, true] {
        let mut fixture = Fixture::new("healthy").await;
        configure(&mut fixture.host, Arc::new(TelegramTransport), |_| {});
        let mut events = replies(&fixture).await;
        let sent=fixture.action(json!({"action":"send","destination":{"kind":"fixed_session","session":fixture.b.session_id()},"intent":"question","content":"answer me","policy":{"revision":1,"intermediate":null,"final_result":{"kind":"fixed_session","session":fixture.a.session_id(),"display":{"connector":"telegram","channel":"7"}}}})).await;
        let id: MessageId = from_value(sent["message_id"].clone()).unwrap();
        if cancel {
            fixture
                .action(json!({"action":"cancel","message_id":id}))
                .await;
        } else {
            fixture.host.panel_maintenance(&fixture.ctx).await;
            fixture.wait_terminal(&id).await;
        }
        for _ in 0..5 {
            fixture.host.panel_maintenance(&fixture.ctx).await;
        }
        let frame = one(&mut events).await;
        assert!(frame.context.get::<ReportAttempt>().is_some());
        assert!(frame.context.get::<OrdinarySessionOutput>().is_none());
        assert!(frame
            .payload_as::<String>()
            .unwrap()
            .starts_with("Message from session"));
        assert!(timeout(Duration::from_millis(30), events.next())
            .await
            .is_err());
        let records = fixture
            .service
            .history()
            .load_session(&fixture.a.journal())
            .await
            .unwrap();
        assert!(
            records
                .iter()
                .any(|record| record.content.contains("received.v1")
                    && record.content.contains("\\\"intent\\\":\\\"response\\\""))
                || records
                    .iter()
                    .any(|record| record.content.contains("\"intent\":\"response\"")
                        && record.content.contains("received.v1"))
        );
    }
}
