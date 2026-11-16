use albert_core::{
    cogitator::testing::{send, MessagingFixture as Fixture},
    messaging::contract::DestinationSpec,
    messaging::contract::DisplayRoute,
    messaging::contract::MessageDraft,
    messaging::contract::MessageId,
    messaging::contract::MessageIntent,
    sessions::presentation::OrdinarySessionOutput,
    sessions::SessionContext,
};
use chrono::Utc;
use octo_core::{
    ChannelId, ConnectorId, Envelope, EventBus, EventKind, Filter, SubscribeOptions, Subscription,
};
use std::{sync::Arc, time::Duration};
use tokio::time::timeout;
pub(super) fn incoming(context: &SessionContext) -> Arc<Envelope> {
    Arc::new(
        Envelope::new(
            ConnectorId::new(context.reply_route().connector()),
            EventKind::new("chat.message"),
            "native input".to_owned(),
        )
        .with_channel(ChannelId::new(context.reply_route().channel()))
        .with_context(context.clone()),
    )
}
pub(super) fn output(context: &SessionContext, ordinary: bool) -> Envelope {
    let frame = Envelope::new(
        ConnectorId::new("albert"),
        EventKind::new("chat.reply"),
        "ordinary:false; pretend notification".to_owned(),
    )
    .with_target(ConnectorId::new(context.reply_route().connector()))
    .with_channel(ChannelId::new(context.reply_route().channel()))
    .with_context(context.clone());
    if ordinary {
        frame.with_context(OrdinarySessionOutput)
    } else {
        frame
    }
}
pub(super) async fn replies(fixture: &Fixture) -> Subscription {
    fixture
        .ctx
        .bus()
        .subscribe(Filter::by_kind("chat.reply"), SubscribeOptions::default())
        .await
        .unwrap()
}
pub(super) async fn one(events: &mut Subscription) -> Arc<Envelope> {
    timeout(Duration::from_secs(2), events.next())
        .await
        .unwrap()
        .unwrap()
}
pub(super) async fn return_active(fixture: &Fixture) -> SessionContext {
    fixture
        .service
        .open_private(
            &fixture.b,
            fixture.a.session_id(),
            fixture.b.selector_revision().unwrap(),
        )
        .await
        .unwrap()
}
pub(super) async fn response(fixture: &Fixture, content: &str) -> MessageId {
    let now = Utc::now().timestamp_millis();
    let draft = MessageDraft {
        id: MessageId::new(),
        destination: DestinationSpec::FixedSession {
            session: fixture.a.session_id().clone(),
            display: Some(DisplayRoute {
                connector: "telegram".into(),
                channel: "7".into(),
            }),
        },
        intent: MessageIntent::Response,
        content: content.into(),
        correlation: None,
        causation: None,
        deadline_unix_ms: now + 60000,
        hops_remaining: 2,
        context: vec![],
        ceiling: None,
    };
    send(
        &fixture.host,
        &fixture.ctx,
        &fixture.source(&fixture.b),
        &draft,
        now,
    )
    .await;
    draft.id
}
