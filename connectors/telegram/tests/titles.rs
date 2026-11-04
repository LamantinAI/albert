use std::{sync::Arc, time::Duration};

use albert_connector_telegram::{SessionTelegramPolicy, TelegramTransport};
use albert_core::cogitator::testing::{authority, configure, message, SessionFixture as Fixture};
use octo_connector_telegram::{IngressDecision, NativeFrame, TelegramHostPolicy, TurnFeedback};
use octo_core::{ChannelId, ConnectorId, Envelope, EventBus, EventKind, Filter, SubscribeOptions};
use tokio::time::timeout;
#[tokio::test]
async fn rename_keeps_running_native_attempt_and_presentation_intact() {
    let mut fixture = Fixture::with_model("never-rename").await;
    configure(&mut fixture.host, Arc::new(TelegramTransport), |config| {
        config.stream_status = true
    });
    let original = fixture.context("telegram", "7", "7").await;
    let mut events = fixture
        .ctx
        .bus()
        .subscribe(Filter::by_kind("chat.typing"), SubscribeOptions::default())
        .await
        .unwrap();
    fixture
        .host
        .clone()
        .panel_handle(
            message("telegram", "7", "7", "continue work", false, true),
            &fixture.ctx,
        )
        .await;
    let typing = timeout(Duration::from_secs(1), events.next())
        .await
        .unwrap()
        .unwrap();
    let token = typing.context.get::<TurnFeedback>().unwrap().clone();
    fixture.request_count(1).await;
    let policy = SessionTelegramPolicy::new(fixture.service.clone(), authority(&fixture.host));
    let frame = NativeFrame {
        connector: ConnectorId::new("telegram"),
        bot_id: 99,
        update_id: 500,
        message_id: 500,
        date: 1_800_000_500,
        chat_id: 7,
        actor_id: Some(7),
        chat_type: "private".into(),
        command_text: Some("/rename Running SQLite study".into()),
        digest: "rename-current".into(),
        media: false,
    };
    let decision = policy
        .admit(
            frame,
            Envelope::new(
                ConnectorId::new("telegram"),
                EventKind::new("chat.message"),
                "/rename Running SQLite study".to_owned(),
            )
            .with_channel(ChannelId::new("7")),
        )
        .await
        .unwrap();
    assert!(matches!(decision, IngressDecision::Control { .. }));
    assert!(token.is_open());
    assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
    let renamed = fixture
        .service
        .get(&original, original.session_id())
        .await
        .unwrap();
    assert_eq!(renamed.title, "Running SQLite study");
    fixture.host.panel_stop(&fixture.ctx).await;
    assert!(!token.is_open());
}
