use std::{sync::Arc, time::Duration};

use albert_connector_telegram::{SessionTelegramPolicy, TelegramTransport};
use albert_core::{
    cogitator::testing::message,
    cogitator::testing::SessionFixture as Fixture,
    cogitator::testing::{authority, configure, detach},
    history::HistoryStore,
};
use octo_connector_telegram::{
    CallbackFrame, CallbackMessage, IngressDecision, InlineKeyboard, NativeFrame,
    TelegramHostPolicy, TurnFeedback,
};
use octo_core::{ChannelId, ConnectorId, Envelope, EventBus, EventKind, Filter, SubscribeOptions};
use tokio::time::timeout;

#[tokio::test]
async fn picker_reopens_running_session_with_fresh_progress_and_next_native_input_stays_there() {
    let mut fixture = Fixture::with_model("never-picker").await;
    configure(&mut fixture.host, Arc::new(TelegramTransport), |config| {
        config.stream_status = true
    });
    let a = fixture.context("telegram", "7", "7").await;
    let mut typing = fixture
        .ctx
        .bus()
        .subscribe(Filter::by_kind("chat.typing"), SubscribeOptions::default())
        .await
        .unwrap();
    fixture
        .host
        .clone()
        .panel_handle(
            message("telegram", "7", "7", "keep the original work", false, true),
            &fixture.ctx,
        )
        .await;
    let original = timeout(Duration::from_secs(1), typing.next())
        .await
        .unwrap()
        .unwrap();
    let old = original.context.get::<TurnFeedback>().unwrap().clone();
    fixture.request_count(1).await;
    let b = fixture
        .service
        .create_private(&a, "Another view")
        .await
        .unwrap();
    let detached = fixture.service.presentations().event_for(&a, &b).unwrap();
    detach(&fixture.host, &detached, &fixture.ctx).await;
    assert!(!old.is_open());
    let policy = SessionTelegramPolicy::new(fixture.service.clone(), authority(&fixture.host));
    let native = NativeFrame {
        connector: ConnectorId::new("telegram"),
        bot_id: 99,
        update_id: 500,
        message_id: 500,
        date: 1_800_000_500,
        chat_id: 7,
        actor_id: Some(7),
        chat_type: "private".into(),
        command_text: Some("/sessions".into()),
        digest: "picker-menu".into(),
        media: false,
    };
    let decision = policy
        .admit(
            native,
            Envelope::new(
                ConnectorId::new("telegram"),
                EventKind::new("chat.message"),
                "/sessions".to_string(),
            )
            .with_channel(ChannelId::new("7")),
        )
        .await
        .unwrap();
    let reply = match decision {
        IngressDecision::Control { post_events, .. } => post_events
            .into_iter()
            .find(|event| event.context.get::<InlineKeyboard>().is_some())
            .unwrap(),
        _ => panic!("picker"),
    };
    let button = reply
        .context
        .get::<InlineKeyboard>()
        .unwrap()
        .rows()
        .iter()
        .flatten()
        .find(|button| button.label().contains("Standard"))
        .unwrap();
    let callback = CallbackFrame {
        connector: ConnectorId::new("telegram"),
        bot_id: 99,
        update_id: 501,
        query_id: "open-running".into(),
        actor_id: 7,
        message: Some(CallbackMessage {
            chat_id: 7,
            message_id: 600,
            chat_type: "private".into(),
            accessible: true,
            sender_id: Some(99),
        }),
        inline_message_id: None,
        data: button.data().into(),
    };
    let (_, _, events) = policy.callback(callback).await.unwrap().into_parts();
    for event in events {
        if event.kind.as_str().starts_with("albert.presentation.") {
            fixture
                .host
                .clone()
                .panel_handle(Arc::new(event), &fixture.ctx)
                .await;
        }
    }
    let attached = timeout(Duration::from_secs(1), typing.next())
        .await
        .unwrap()
        .unwrap();
    let fresh = attached.context.get::<TurnFeedback>().unwrap().clone();
    assert_ne!(fresh.id(), old.id());
    assert!(fresh.is_open());
    assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
    assert_eq!(
        fixture.context("telegram", "7", "7").await.session_id(),
        a.session_id()
    );
    fixture
        .host
        .clone()
        .panel_handle(
            message(
                "telegram",
                "7",
                "7",
                "new human steering after click",
                false,
                true,
            ),
            &fixture.ctx,
        )
        .await;
    fixture.request_count(2).await;
    assert!(!fresh.is_open());
    let records = fixture.router.load(&a.root_history_key()).await;
    assert!(records
        .iter()
        .any(|turn| turn.content.contains("keep the original work")));
    assert!(records
        .iter()
        .any(|turn| turn.content.contains("new human steering after click")));
    assert!(fixture.router.load(&b.root_history_key()).await.is_empty());
    fixture.host.panel_stop(&fixture.ctx).await;
}
