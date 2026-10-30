//! Authenticated direct-resource inputs retain receipts and restricted tools
//! through the same runtime used by selector-bound adapters.
use std::{sync::Arc, time::Duration};

use octo_connector_telegram::{TurnFeedback, TURN_END};
use octo_core::{
    ChannelId, ChannelMetadata, CogitatorContext, ConnectorCapabilities, ConnectorId,
    ConnectorInfo, Envelope, EventBus, EventKind, Filter, SubscribeOptions, Subscription,
};
use tokio::time::timeout;

use super::Fixture;
use crate::{
    cogitator::compaction::CompactOptions,
    db::repo::identity::bind,
    service::session::web::WebActor,
    sessions::{acceptance::SourceReceipt, actor::ActorToolPolicy, SessionContext, UserId},
};

fn configure_web(fixture: &mut Fixture) {
    let mut connectors = fixture.ctx.connectors().to_vec();
    connectors.push(ConnectorInfo {
        id: ConnectorId::new("web"),
        capabilities: ConnectorCapabilities::bidirectional()
            .with_emit_kinds([EventKind::new("chat.message")]),
    });
    fixture.ctx =
        CogitatorContext::new(fixture.ctx.shutdown.clone(), fixture.ctx.bus(), connectors);
}
async fn actor(fixture: &Fixture, account: &str, tools: ActorToolPolicy) -> WebActor {
    let mut tx = fixture.service.history().pool().begin().await.unwrap();
    bind(&mut tx, "web", account, "web-fixture-user")
        .await
        .unwrap();
    tx.commit().await.unwrap();
    WebActor {
        account_id: account.into(),
        user_id: UserId::parse("web-fixture-user").unwrap(),
        tools,
    }
}
fn envelope(context: &SessionContext, key: &str, text: &str) -> Envelope {
    Envelope::new(
        ConnectorId::new("web"),
        EventKind::new("chat.message"),
        text.to_owned(),
    )
    .with_channel(ChannelId::new(context.reply_route().channel()))
    .with_channel_metadata(
        ChannelMetadata::default()
            .with_tag("chat_type", "private")
            .with_tag("sender_id", context.identity().unwrap().actor())
            .with_tag("message_id", key)
            .with_tag("role", if context.chat_only() { "user" } else { "owner" }),
    )
    .with_context(context.clone())
}
async fn next(events: &mut Subscription, kind: &str) -> Arc<Envelope> {
    timeout(Duration::from_secs(3), async {
        loop {
            let event = events.next().await.unwrap();
            if event.kind.as_str() == kind {
                return event;
            }
        }
    })
    .await
    .unwrap()
}
fn token(event: &Envelope) -> TurnFeedback {
    event.context.get::<TurnFeedback>().unwrap().clone()
}

#[tokio::test]
async fn direct_web_input_has_receipt_progress_and_chat_only_tools_without_a_selector() {
    let mut fixture = Fixture::new().await;
    configure_web(&mut fixture);
    Arc::get_mut(&mut fixture.host)
        .unwrap()
        .config
        .stream_status = true;
    let user = actor(&fixture, "web-user", ActorToolPolicy::ChatOnly).await;
    let session = fixture
        .service
        .web_create(&user, "Web conversation", false)
        .await
        .unwrap();
    let context = fixture
        .service
        .web_context(&user, &session.id)
        .await
        .unwrap();
    assert!(context.selector_revision().is_none());
    let mut events = fixture
        .ctx
        .bus()
        .subscribe(
            Filter::by_target(ConnectorId::new("web")),
            SubscribeOptions::default(),
        )
        .await
        .unwrap();
    let (receipt, accepted) = SourceReceipt::channel();
    fixture
        .host
        .clone()
        .handle(
            Arc::new(
                envelope(&context, "web-first", "A scoped browser question").with_context(receipt),
            ),
            &fixture.ctx,
        )
        .await;
    let receipt = timeout(Duration::from_secs(1), accepted)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(receipt.record_ids.len(), 1);
    let typing = next(&mut events, "chat.typing").await;
    assert!(token(&typing).is_open());
    let reply = next(&mut events, "chat.reply").await;
    assert!(!token(&typing).is_open());
    assert_eq!(
        reply.context.get::<SessionContext>().unwrap().session_id(),
        &session.id
    );
    let requests = fixture.server.requests.lock().unwrap();
    let tools = requests[0]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 6);
    assert!(tools.iter().all(|tool| tool["function"]["name"]
        .as_str()
        .unwrap()
        .starts_with("scratchpad_")));
}
#[tokio::test]
async fn compaction_attenuates_owner_continuation_and_keeps_fresh_web_progress_and_original_route()
{
    let mut fixture = Fixture::with_model("feedback-phases").await;
    configure_web(&mut fixture);
    Arc::get_mut(&mut fixture.host)
        .unwrap()
        .config
        .stream_status = true;
    let owner = actor(&fixture, "web-owner", ActorToolPolicy::ExistingHost).await;
    let user = actor(&fixture, "web-user", ActorToolPolicy::ChatOnly).await;
    let session = fixture
        .service
        .web_create(&owner, "Web work", false)
        .await
        .unwrap();
    let original = fixture
        .service
        .web_context(&owner, &session.id)
        .await
        .unwrap();
    let restricted = fixture
        .service
        .web_context(&user, &session.id)
        .await
        .unwrap();
    let mut events = fixture
        .ctx
        .bus()
        .subscribe(
            Filter::by_target(ConnectorId::new("web")),
            SubscribeOptions::default(),
        )
        .await
        .unwrap();
    let (receipt, accepted) = SourceReceipt::channel();
    fixture
        .host
        .clone()
        .handle(
            Arc::new(
                envelope(&original, "owner-work", "Continue this browser task")
                    .with_context(receipt),
            ),
            &fixture.ctx,
        )
        .await;
    timeout(Duration::from_secs(1), accepted)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let initial = next(&mut events, "chat.typing").await;
    fixture.request_count(1).await;
    fixture
        .host
        .clone()
        .spawn_compact(
            Arc::new(envelope(&restricted, "compact-work", "/compact")),
            &fixture.ctx,
            CompactOptions::default(),
        )
        .await;
    let ended = next(&mut events, TURN_END).await;
    assert_eq!(token(&ended).id(), token(&initial).id());
    let compact = next(&mut events, "chat.typing").await;
    let compact_end = next(&mut events, TURN_END).await;
    assert_eq!(token(&compact).id(), token(&compact_end).id());
    let resumed = next(&mut events, "chat.typing").await;
    assert_ne!(token(&resumed).id(), token(&initial).id());
    let reply = next(&mut events, "chat.reply").await;
    assert_eq!(reply.channel, initial.channel);
    assert_eq!(
        reply.context.get::<SessionContext>().unwrap().tool_policy(),
        ActorToolPolicy::ChatOnly
    );
    assert!(!token(&resumed).is_open());
    let requests = fixture.server.requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    let tools = requests[2]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 6);
    assert!(tools.iter().all(|tool| tool["function"]["name"]
        .as_str()
        .unwrap()
        .starts_with("scratchpad_")));
}
