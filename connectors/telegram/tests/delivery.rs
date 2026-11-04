use std::{sync::Arc, time::Duration};

use albert_connector_telegram::{SessionTelegramPolicy, TelegramTransport};
use albert_core::{
    cogitator::testing::delivery_pending,
    cogitator::testing::{authority, configure, MessagingFixture as Fixture},
    messaging::{store::DeliveryState, transport::ReportAttempt},
    sessions::{transport::OutputScope, SessionContext},
};
use octo_approval::Ingress;
use octo_connector_telegram::{DeliveryReport, DeliveryStatus, TelegramHostPolicy};
use octo_core::{EventBus, Filter, SubscribeOptions};
use octo_history::SessionStore;
use tokio::time::{sleep, timeout};
#[tokio::test]
async fn selected_native_final_has_one_full_body_and_fresh_attempt_scope() {
    let mut fixture = Fixture::new("message-native-delivery").await;
    configure(&mut fixture.host, Arc::new(TelegramTransport), |_| {});
    let active = fixture
        .service
        .open_private(
            &fixture.b,
            fixture.a.session_id(),
            fixture.b.selector_revision().unwrap(),
        )
        .await
        .unwrap();
    let mut replies = fixture
        .ctx
        .bus()
        .subscribe(Filter::by_kind("chat.reply"), SubscribeOptions::default())
        .await
        .unwrap();
    fixture
        .native(
            &active,
            &format!(
                "Route final result POLICY_TARGET:{}",
                fixture.b.session_id()
            ),
        )
        .await;
    let receipt = timeout(Duration::from_secs(3), replies.next())
        .await
        .unwrap()
        .unwrap();
    let text = receipt.payload_as::<String>().unwrap();
    assert!(
        text.contains("queued for the selected destination"),
        "{text}"
    );
    assert!(!text.contains("answer from message-native-delivery"));
    assert!(receipt.context.get::<ReportAttempt>().is_none());
    for _ in 0..3 {
        fixture.host.panel_maintenance(&fixture.ctx).await;
    }
    let outbound = timeout(Duration::from_secs(3), replies.next())
        .await
        .unwrap()
        .unwrap();
    let attempt = outbound.context.get::<ReportAttempt>().unwrap();
    assert!(outbound
        .payload_as::<String>()
        .unwrap()
        .contains("answer from message-native-delivery"));
    assert!(outbound.context.get::<Ingress>().is_none());
    assert_eq!(
        outbound
            .context
            .get::<SessionContext>()
            .unwrap()
            .session_id(),
        fixture.b.session_id()
    );
    let scope = outbound.context.get::<OutputScope>().unwrap();
    assert!(!scope.scope.ephemeral);
    assert!(scope.scope.cleanup_scope.is_none());
    assert_eq!(scope.attempt_id.as_deref(), Some(attempt.token.as_str()));
    let status = fixture.status(&attempt.message).await;
    assert_eq!(status.delivery, DeliveryState::Started);
    let policy = SessionTelegramPolicy::new(fixture.service.clone(), authority(&fixture.host));
    policy.delivered(
        &outbound,
        &DeliveryReport {
            scope: policy.output_scope(&outbound),
            chat_id: 7,
            message_ids: vec![41],
            status: DeliveryStatus::Sent,
        },
    );
    timeout(Duration::from_secs(2), async {
        loop {
            if !fixture
                .status(&attempt.message)
                .await
                .physical
                .observations
                .is_empty()
            {
                break;
            }
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        fixture.status(&attempt.message).await.delivery,
        DeliveryState::Started
    );
    policy.delivered(
        &outbound,
        &DeliveryReport {
            scope: policy.output_scope(&outbound),
            chat_id: 7,
            message_ids: vec![],
            status: DeliveryStatus::Unknown,
        },
    );
    timeout(Duration::from_secs(2), async {
        loop {
            if fixture.status(&attempt.message).await.delivery == DeliveryState::Unknown {
                break;
            }
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert!(!delivery_pending(&fixture.host).await);
    let source = fixture
        .service
        .history()
        .load_session(&fixture.a.journal())
        .await
        .unwrap();
    assert!(source.iter().any(|turn| turn
        .content
        .starts_with("answer from message-native-delivery")));
}
