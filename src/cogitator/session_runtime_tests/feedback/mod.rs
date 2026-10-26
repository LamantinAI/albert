use std::{sync::Arc, time::Duration};

use octo_connector_telegram::{TurnFeedback, TURN_END};
use octo_core::{ConnectorId, Envelope, EventBus, Filter, SubscribeOptions, Subscription};
use tokio::time::timeout;

use super::{message, Fixture};
use crate::history::{HistoryStore, Turn};

async fn next(subscription: &mut Subscription, kind: &str) -> Arc<Envelope> {
    timeout(Duration::from_secs(5), async {
        loop {
            let event = subscription.next().await.expect("feedback subscription");
            if event.kind.as_str() == kind {
                return event;
            }
        }
    })
    .await
    .expect("feedback event before deadline")
}

fn token(event: &Envelope) -> TurnFeedback {
    event
        .context
        .get::<TurnFeedback>()
        .expect("native feedback marker")
        .clone()
}

async fn events(fixture: &Fixture, connector: &str) -> Subscription {
    fixture
        .ctx
        .bus()
        .subscribe(
            Filter::by_target(ConnectorId::new(connector)),
            SubscribeOptions::default(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn cancel_ends_the_original_feedback_before_acknowledging_a_different_request() {
    let fixture = Fixture::new().await;
    let mut output = events(&fixture, "telegram").await;
    fixture
        .host
        .clone()
        .handle(
            message("telegram", "7", "7", "start a task", false, true),
            &fixture.ctx,
        )
        .await;
    let typing = next(&mut output, "chat.typing").await;
    fixture
        .host
        .clone()
        .handle(
            message("telegram", "7", "7", "/cancel", false, true),
            &fixture.ctx,
        )
        .await;
    let ended = next(&mut output, TURN_END).await;
    assert_eq!(token(&ended).id(), token(&typing).id());
    assert!(!token(&typing).is_open());
    assert_eq!(ended.target, typing.target);
    assert_eq!(ended.channel, typing.channel);
    let acknowledgement = next(&mut output, "chat.reply").await;
    assert!(acknowledgement.context.get::<TurnFeedback>().is_none());
    fixture.host.stop_turns(&fixture.ctx).await;
}

#[tokio::test]
async fn interruption_from_a_linked_connector_closes_the_old_physical_route_only() {
    let fixture = Fixture::new().await;
    let original = fixture.context("telegram", "7", "7").await;
    let console = fixture.context("console", "local", "stdin").await;
    fixture
        .service
        .open_private(
            &console,
            original.session_id(),
            console.selector_revision().unwrap(),
        )
        .await
        .unwrap();
    let mut telegram = events(&fixture, "telegram").await;
    let mut console_events = events(&fixture, "console").await;
    fixture
        .host
        .clone()
        .handle(
            message("telegram", "7", "7", "first", false, true),
            &fixture.ctx,
        )
        .await;
    let old = next(&mut telegram, "chat.typing").await;
    fixture
        .host
        .clone()
        .handle(
            message("console", "local", "stdin", "new direction", false, true),
            &fixture.ctx,
        )
        .await;
    let ended = next(&mut telegram, TURN_END).await;
    assert_eq!(token(&old).id(), token(&ended).id());
    assert!(!token(&old).is_open());
    let current = next(&mut console_events, "chat.typing").await;
    assert_ne!(token(&old).id(), token(&current).id());
    let end_current = next(&mut console_events, TURN_END).await;
    assert_eq!(token(&current).id(), token(&end_current).id());
    let reply = next(&mut console_events, "chat.reply").await;
    assert_eq!(token(&reply).id(), token(&current).id());
    assert_eq!(reply.channel.as_ref().unwrap().as_str(), "stdin");
    fixture.host.stop_turns(&fixture.ctx).await;
}

#[tokio::test]
async fn compaction_and_resumed_work_use_distinct_feedback_lifetimes() {
    let fixture = Fixture::with_model("feedback-phases").await;
    let context = fixture.context("telegram", "7", "7").await;
    fixture
        .router
        .append(
            &context.root_history_key(),
            &[Turn::user("old question"), Turn::assistant("old answer")],
        )
        .await
        .unwrap();
    let mut output = events(&fixture, "telegram").await;
    fixture
        .host
        .clone()
        .handle(
            message("telegram", "7", "7", "keep working", false, true),
            &fixture.ctx,
        )
        .await;
    let first = next(&mut output, "chat.typing").await;
    fixture.request_count(1).await;
    fixture
        .host
        .clone()
        .handle(
            message("telegram", "7", "7", "/compact", false, true),
            &fixture.ctx,
        )
        .await;
    let first_end = next(&mut output, TURN_END).await;
    assert_eq!(token(&first).id(), token(&first_end).id());
    let compact = next(&mut output, "chat.typing").await;
    let compact_end = next(&mut output, TURN_END).await;
    assert_eq!(token(&compact).id(), token(&compact_end).id());
    let resumed = next(&mut output, "chat.typing").await;
    assert_ne!(token(&resumed).id(), token(&first).id());
    assert_ne!(token(&resumed).id(), token(&compact).id());
    let resumed_end = next(&mut output, TURN_END).await;
    assert_eq!(token(&resumed).id(), token(&resumed_end).id());
    let reply = next(&mut output, "chat.reply").await;
    assert_eq!(token(&reply).id(), token(&resumed).id());
    fixture.host.stop_turns(&fixture.ctx).await;
}

#[tokio::test]
async fn idle_compaction_can_be_cancelled_without_a_task_continuation() {
    let fixture = Fixture::with_model("feedback-phases").await;
    let context = fixture.context("telegram", "7", "7").await;
    fixture
        .router
        .append(
            &context.root_history_key(),
            &[Turn::user("old question"), Turn::assistant("old answer")],
        )
        .await
        .unwrap();
    let mut output = events(&fixture, "telegram").await;
    fixture
        .host
        .clone()
        .handle(
            message("telegram", "7", "7", "/compact", false, true),
            &fixture.ctx,
        )
        .await;
    let compact = next(&mut output, "chat.typing").await;
    fixture
        .host
        .clone()
        .handle(
            message("telegram", "7", "7", "/cancel", false, true),
            &fixture.ctx,
        )
        .await;
    let ended = next(&mut output, TURN_END).await;
    assert_eq!(token(&compact).id(), token(&ended).id());
    assert!(!token(&compact).is_open());
    next(&mut output, "chat.reply").await;
    fixture.host.stop_turns(&fixture.ctx).await;
}
