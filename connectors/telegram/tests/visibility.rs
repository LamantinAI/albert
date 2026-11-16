use albert_connector_telegram::{SessionTelegramPolicy, TelegramTransport};
use albert_core::{
    cogitator::testing::{
        authority, configure, feedback, reply as emit_reply, MessagingFixture as Fixture,
    },
    db::entity::PrivateRouteKey,
    sessions::presentation::OrdinarySessionOutput,
    sessions::presentation::ScheduledNotification,
    sessions::Admission,
    sessions::AdmissionScope,
    sessions::ExternalIdentity,
    sessions::ReplyRoute,
};
use octo_connector_telegram::{TelegramHostPolicy, TurnFeedback};
use octo_history::SessionStore;
use std::{sync::Arc, time::Duration};
use tokio::{time::sleep, time::timeout};
#[allow(dead_code)]
#[path = "common/visibility.rs"]
mod fixture;
use self::fixture::{incoming, one, output, replies, return_active};

#[tokio::test]
async fn native_errors_and_replies_are_active_only_but_status_cutoff_stays_closed_on_return() {
    let mut fixture = Fixture::new("healthy").await;
    configure(&mut fixture.host, Arc::new(TelegramTransport), |_| {});
    let mut events = replies(&fixture).await;
    let old = incoming(&fixture.a);
    emit_reply(&fixture.host, &old, "hidden old error".into(), &fixture.ctx).await;
    assert!(timeout(Duration::from_millis(30), events.next())
        .await
        .is_err());
    let active = return_active(&fixture).await;
    assert!(fixture.service.presentations().active(&fixture.a));
    assert!(fixture.service.presentations().detached(&fixture.a));
    let old_token = feedback(&fixture.host, &old, "old-a-status");
    assert!(!old_token.context.get::<TurnFeedback>().unwrap().is_open());
    emit_reply(
        &fixture.host,
        &old,
        "finishes after returning to A".into(),
        &fixture.ctx,
    )
    .await;
    let emitted = one(&mut events).await;
    assert_eq!(
        emitted.payload_as::<String>().unwrap(),
        "finishes after returning to A"
    );
    assert!(emitted.context.get::<OrdinarySessionOutput>().is_some());
    let policy = SessionTelegramPolicy::new(fixture.service.clone(), authority(&fixture.host));
    assert!(policy.allow_delivery(&output(&fixture.a, true)));
    let _ = fixture
        .service
        .open_private(
            &active,
            fixture.b.session_id(),
            active.selector_revision().unwrap(),
        )
        .await
        .unwrap();
    assert!(!policy.allow_delivery(&emitted));
    assert!(!policy.allow_delivery(&output(&fixture.a, true)));
    assert!(policy.allow_delivery(&output(&fixture.a, false))); // explicit/approval path is not inferred ordinary
    let notification = Arc::new((*old).clone().with_context(ScheduledNotification));
    emit_reply(
        &fixture.host,
        &notification,
        "scheduled reminder".into(),
        &fixture.ctx,
    )
    .await;
    assert_eq!(
        one(&mut events).await.payload_as::<String>().unwrap(),
        "scheduled reminder"
    );
}
#[tokio::test]
async fn current_cache_ignores_old_reads_and_replay_does_not_reselect_bound_session() {
    let mut fixture = Fixture::new("healthy").await;
    configure(&mut fixture.host, Arc::new(TelegramTransport), |_| {});
    let key = PrivateRouteKey {
        connector_id: "telegram".into(),
        channel_id: "7".into(),
        user_id: fixture.a.private_user_id().unwrap().as_str().into(),
    };
    fixture
        .service
        .presentations()
        .observe(&key, fixture.a.session_id(), 1);
    assert!(!fixture.service.presentations().active(&fixture.a));
    assert!(fixture.service.presentations().active(&fixture.b));
    let replay = fixture
        .service
        .context_for_message(
            Admission::new(
                ExternalIdentity::new("telegram", "7").unwrap(),
                ReplyRoute::new("telegram", "7", None).unwrap(),
                AdmissionScope::Private,
                "host",
            )
            .unwrap(),
            fixture.a.session_id(),
        )
        .await
        .unwrap();
    assert_eq!(replay.session_id(), fixture.a.session_id());
    assert!(!fixture.service.presentations().active(&replay));
    assert!(fixture.service.presentations().active(&fixture.b));
    let group = fixture
        .service
        .admit(
            Admission::new(
                ExternalIdentity::new("telegram", "7").unwrap(),
                ReplyRoute::new("telegram", "-42", None).unwrap(),
                AdmissionScope::Group,
                "host",
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let policy = SessionTelegramPolicy::new(fixture.service.clone(), authority(&fixture.host));
    assert!(policy.allow_delivery(&output(&group, true)));
}
#[tokio::test]
async fn inactive_native_result_is_saved_but_only_active_a_receives_the_later_implicit_reply() {
    let mut fixture = Fixture::new("visibility-native").await;
    configure(&mut fixture.host, Arc::new(TelegramTransport), |_| {});
    let mut events = replies(&fixture).await;
    fixture
        .native(&fixture.b, "Produce main research report")
        .await;
    fixture.wait_requests(1).await;
    let _active = return_active(&fixture).await;
    timeout(Duration::from_secs(3), async {
        loop {
            let records = fixture
                .service
                .history()
                .load_session(&fixture.b.journal())
                .await
                .unwrap();
            if records
                .iter()
                .any(|record| record.content.starts_with("Native full report with table"))
            {
                break;
            }
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert!(timeout(Duration::from_millis(30), events.next())
        .await
        .is_err());
    let id = fixture.question("What is your current status?").await;
    fixture.host.panel_maintenance(&fixture.ctx).await;
    fixture.wait_terminal(&id).await;
    for _ in 0..5 {
        fixture.host.panel_maintenance(&fixture.ctx).await;
    }
    let frame = one(&mut events).await;
    let text = frame.payload_as::<String>().unwrap();
    assert!(text.starts_with("Reply from session"));
    assert!(text.contains("\n\nShort inter-session response"));
    assert!(!text.contains("Native full report with table"));
    assert!(frame.context.get::<OrdinarySessionOutput>().is_some());
    assert_eq!(fixture.server.requests.lock().unwrap().len(), 2);
    fixture.host.panel_stop(&fixture.ctx).await;
}
