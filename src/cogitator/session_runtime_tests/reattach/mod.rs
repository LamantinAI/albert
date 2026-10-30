use std::{slice::from_ref, sync::Arc, time::Duration};

use octo_connector_telegram::{TurnFeedback, TURN_END};
use octo_core::{
    ConnectorId, Envelope, EventBus, EventKind, Filter, SubscribeOptions, Subscription,
};
use serde_json::{json, to_string};
use tokio::time::timeout;

use super::{message, Fixture};
use crate::{
    cogitator::compaction::CompactOptions,
    history::{to_messages, Turn},
    sessions::{presentation::PresentationOpened, SessionContext},
    status::StatusFeed,
};

async fn next(events: &mut Subscription, kind: &str) -> Arc<Envelope> {
    timeout(Duration::from_secs(4), async {
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
async fn opened(fixture: &Fixture, context: &SessionContext) {
    fixture
        .host
        .clone()
        .handle(
            Arc::new(
                Envelope::new(
                    ConnectorId::new("albert.sessions"),
                    EventKind::new("albert.presentation.opened"),
                    json!({}),
                )
                .with_context(PresentationOpened(context.clone())),
            ),
            &fixture.ctx,
        )
        .await;
}
async fn feed(fixture: &Fixture, context: &SessionContext) -> StatusFeed {
    let state = fixture
        .host
        .turns
        .lock()
        .unwrap()
        .get(&("session".into(), context.root_history_key()))
        .unwrap()
        .clone();
    let feed = state.lock().await.presentation_feed().unwrap();
    feed
}

#[tokio::test]
async fn reopen_attaches_fresh_status_without_restarting_work_or_changing_execution() {
    let mut fixture = Fixture::with_model("never-reattach").await;
    Arc::get_mut(&mut fixture.host)
        .unwrap()
        .config
        .stream_status = true;
    let a = fixture.context("telegram", "7", "7").await;
    let mut events = fixture
        .ctx
        .bus()
        .subscribe(
            Filter::by_target(ConnectorId::new("telegram")),
            SubscribeOptions::default(),
        )
        .await
        .unwrap();
    fixture
        .host
        .clone()
        .handle(
            message("telegram", "7", "7", "keep working", false, true),
            &fixture.ctx,
        )
        .await;
    let original = next(&mut events, "chat.typing").await;
    fixture.request_count(1).await;
    let current_feed = feed(&fixture, &a).await;
    current_feed.start_external("host-call", "search", json!({"q":"keep"}));
    current_feed.finish_external("host-call", "retained tool evidence");
    let diagnostic = current_feed.request_id();
    let b = fixture.service.create_private(&a, "B").await.unwrap();
    let old_detach = fixture.service.presentations().event_for(&a, &b).unwrap();
    fixture
        .host
        .detach_presentation(&old_detach, &fixture.ctx)
        .await;
    let old_end = next(&mut events, TURN_END).await;
    assert_eq!(token(&old_end).id(), token(&original).id());
    assert!(!token(&original).is_open());
    let back = fixture
        .service
        .open_private(&b, a.session_id(), b.selector_revision().unwrap())
        .await
        .unwrap();
    opened(&fixture, &back).await;
    let fresh = next(&mut events, "chat.typing").await;
    assert_ne!(token(&fresh).id(), token(&original).id());
    assert_eq!(
        fresh
            .context
            .get::<SessionContext>()
            .unwrap()
            .selector_revision(),
        back.selector_revision()
    );
    assert_eq!(
        next(&mut events, "chat.status")
            .await
            .payload_as::<String>()
            .unwrap(),
        "Work is still running"
    );
    opened(&fixture, &back).await;
    fixture
        .host
        .detach_presentation(&old_detach, &fixture.ctx)
        .await;
    assert!(token(&fresh).is_open());
    assert_eq!(current_feed.request_id(), diagnostic);
    assert!(current_feed.is_live());
    assert!(!current_feed.snapshot().is_empty());
    assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
    assert!(timeout(Duration::from_millis(30), events.next())
        .await
        .is_err());
    fixture
        .host
        .clone()
        .handle(
            message("telegram", "7", "7", "/cancel", false, true),
            &fixture.ctx,
        )
        .await;
    let ended = next(&mut events, TURN_END).await;
    assert_eq!(token(&ended).id(), token(&fresh).id());
    assert!(!token(&fresh).is_open());
    let records = fixture.host.history.load(&a.root_history_key()).await;
    assert!(records
        .iter()
        .any(|turn| turn.content.contains("keep working")));
    assert!(records
        .iter()
        .any(|turn| to_string(&to_messages(from_ref(turn)))
            .unwrap()
            .contains("retained tool evidence")));
    opened(&fixture, &back).await;
    assert!(!token(&fresh).is_open());
}

#[tokio::test]
async fn stale_open_and_counterfeit_event_cannot_attach_or_revive_disabled_feed() {
    let fixture = Fixture::with_model("never-reattach-disabled").await;
    let a = fixture.context("telegram", "7", "7").await;
    let mut events = fixture
        .ctx
        .bus()
        .subscribe(
            Filter::by_target(ConnectorId::new("telegram")),
            SubscribeOptions::default(),
        )
        .await
        .unwrap();
    fixture
        .host
        .clone()
        .handle(
            message("telegram", "7", "7", "work", false, true),
            &fixture.ctx,
        )
        .await;
    next(&mut events, "chat.typing").await;
    fixture.request_count(1).await;
    let b = fixture.service.create_private(&a, "B").await.unwrap();
    let detached = fixture.service.presentations().event_for(&a, &b).unwrap();
    fixture
        .host
        .detach_presentation(&detached, &fixture.ctx)
        .await;
    next(&mut events, TURN_END).await;
    opened(&fixture, &a).await; // Stale revision, even though the Session is still live.
    fixture
        .host
        .clone()
        .handle(
            Arc::new(Envelope::new(
                ConnectorId::new("albert.sessions"),
                EventKind::new("albert.presentation.opened"),
                json!({"session_id":a.session_id()}),
            )),
            &fixture.ctx,
        )
        .await;
    let back = fixture
        .service
        .open_private(&b, a.session_id(), b.selector_revision().unwrap())
        .await
        .unwrap();
    opened(&fixture, &back).await; // Config-disabled is not an attachable feed.
    assert!(timeout(Duration::from_millis(30), events.next())
        .await
        .is_err());
    fixture.host.stop_turns(&fixture.ctx).await;
}

#[tokio::test]
async fn binding_snapshots_finish_once_and_reject_other_routes_and_users() {
    let fixture = Fixture::new().await;
    let a = fixture.context("telegram", "7", "7").await;
    let feed = StatusFeed::new(
        fixture.ctx.bus(),
        ConnectorId::new("albert"),
        ConnectorId::new("telegram"),
        None,
    )
    .with_session_context(Some(a.clone()))
    .initially_detached();
    let mut other = a.clone();
    other.reply = fixture.context("console", "local", "stdin").await.reply;
    assert!(!feed.attach_presentation(&other));
    other = a.clone();
    other.user = None;
    assert!(!feed.attach_presentation(&other));
    assert!(feed.attach_presentation(&a));
    let old = feed.presentation_binding().unwrap();
    feed.detach_presentation(|_| true).unwrap();
    assert!(!old.feedback.as_ref().unwrap().is_open());
    assert!(feed.attach_presentation(&a));
    let current = feed.presentation_binding().unwrap();
    assert_ne!(
        old.feedback.as_ref().unwrap().id(),
        current.feedback.as_ref().unwrap().id()
    );
    assert!(feed.finish_presentation().is_some());
    assert!(!current.feedback.unwrap().is_open());
    assert!(feed.finish_presentation().is_none());
    assert!(!feed.attach_presentation(&a));
    assert_eq!(fixture.server.requests.lock().unwrap().len(), 0);
    let records: Vec<Turn> = fixture.host.history.load(&a.root_history_key()).await;
    assert!(records.is_empty());
}

#[tokio::test]
async fn open_before_delayed_detach_retires_stale_binding_and_keeps_new_token() {
    let mut fixture = Fixture::with_model("never-reattach-reordered").await;
    Arc::get_mut(&mut fixture.host)
        .unwrap()
        .config
        .stream_status = true;
    let a = fixture.context("telegram", "7", "7").await;
    let mut events = fixture
        .ctx
        .bus()
        .subscribe(
            Filter::by_target(ConnectorId::new("telegram")),
            SubscribeOptions::default(),
        )
        .await
        .unwrap();
    fixture
        .host
        .clone()
        .handle(
            message("telegram", "7", "7", "work", false, true),
            &fixture.ctx,
        )
        .await;
    let old = next(&mut events, "chat.typing").await;
    let b = fixture.service.create_private(&a, "B").await.unwrap();
    let delayed = fixture.service.presentations().event_for(&a, &b).unwrap();
    let back = fixture
        .service
        .open_private(&b, a.session_id(), b.selector_revision().unwrap())
        .await
        .unwrap();
    assert!(token(&old).is_open());
    opened(&fixture, &back).await;
    let ended = next(&mut events, TURN_END).await;
    assert_eq!(token(&ended).id(), token(&old).id());
    let new = next(&mut events, "chat.typing").await;
    assert_ne!(token(&new).id(), token(&old).id());
    fixture
        .host
        .detach_presentation(&delayed, &fixture.ctx)
        .await;
    assert!(token(&new).is_open());
    assert!(!token(&old).is_open());
    fixture.host.stop_turns(&fixture.ctx).await;
    assert!(!token(&new).is_open());
}

#[tokio::test]
async fn compact_slot_reattaches_real_progress_and_resume_uses_a_new_phase_token() {
    let mut fixture = Fixture::with_model("compact-delayed").await;
    Arc::get_mut(&mut fixture.host)
        .unwrap()
        .config
        .stream_status = true;
    let a = fixture.context("telegram", "7", "7").await;
    fixture
        .host
        .history
        .append(
            &a.root_history_key(),
            &[Turn::user("earlier context ".repeat(100))],
        )
        .await
        .unwrap();
    let mut events = fixture
        .ctx
        .bus()
        .subscribe(
            Filter::by_target(ConnectorId::new("telegram")),
            SubscribeOptions::default(),
        )
        .await
        .unwrap();
    fixture
        .host
        .clone()
        .handle(
            message("telegram", "7", "7", "continue task", false, true),
            &fixture.ctx,
        )
        .await;
    fixture
        .host
        .clone()
        .spawn_compact(
            Arc::new(
                Envelope::new(
                    ConnectorId::new("telegram"),
                    EventKind::new("chat.message"),
                    "/compact".to_owned(),
                )
                .with_context(a.clone()),
            ),
            &fixture.ctx,
            CompactOptions::default(),
        )
        .await;
    fixture.request_count(1).await;
    let compact_feed = feed(&fixture, &a).await;
    let b = fixture.service.create_private(&a, "B").await.unwrap();
    let detached = fixture.service.presentations().event_for(&a, &b).unwrap();
    fixture
        .host
        .detach_presentation(&detached, &fixture.ctx)
        .await;
    assert!(compact_feed.presentation_binding().is_none());
    let back = fixture
        .service
        .open_private(&b, a.session_id(), b.selector_revision().unwrap())
        .await
        .unwrap();
    opened(&fixture, &back).await;
    let attached = compact_feed.presentation_binding().unwrap();
    let attached_token = attached.feedback.unwrap();
    assert!(attached_token.is_open());
    let same = fixture
        .service
        .open_private(&back, back.session_id(), back.selector_revision().unwrap())
        .await
        .unwrap();
    opened(&fixture, &same).await;
    assert_eq!(
        compact_feed
            .presentation_binding()
            .unwrap()
            .feedback
            .unwrap()
            .id(),
        attached_token.id()
    );
    next(&mut events, "chat.reply").await;
    assert!(!attached_token.is_open());
    assert!(compact_feed.presentation_binding().is_none());
    assert_eq!(fixture.server.requests.lock().unwrap().len(), 2);
    assert!(fixture
        .host
        .history
        .context(&a.root_history_key())
        .await
        .unwrap()
        .compact
        .is_some());
    fixture.host.stop_turns(&fixture.ctx).await;
}

#[tokio::test]
async fn saved_promotion_can_reattach_while_unsaved_temporary_cannot() {
    let fixture = Fixture::new().await;
    let a = fixture.context("telegram", "7", "7").await;
    let temporary = fixture.service.start_temporary(&a, None).await.unwrap();
    let feed = StatusFeed::new(
        fixture.ctx.bus(),
        ConnectorId::new("albert"),
        ConnectorId::new("telegram"),
        None,
    )
    .with_session_context(Some(temporary.clone()))
    .initially_detached();
    assert!(!feed.attach_presentation(&temporary));
    let saved = fixture
        .service
        .save_temporary(&temporary, "reattach-save", None)
        .await
        .unwrap();
    let departed = fixture
        .service
        .end_temporary(&saved, None)
        .await
        .unwrap()
        .context;
    let back = fixture
        .service
        .open_private(
            &departed,
            saved.session_id(),
            departed.selector_revision().unwrap(),
        )
        .await
        .unwrap();
    assert!(temporary.is_live());
    assert!(fixture.service.presentations().current(&back));
    assert!(feed.attach_presentation(&back));
    assert!(feed.finish_presentation().is_some());
    assert!(fixture.server.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn native_completion_finishes_fresh_binding_but_retains_original_execution_route() {
    let mut fixture = Fixture::with_model("visibility-native").await;
    Arc::get_mut(&mut fixture.host)
        .unwrap()
        .config
        .stream_status = true;
    let a = fixture.context("telegram", "7", "7").await;
    let mut events = fixture
        .ctx
        .bus()
        .subscribe(
            Filter::by_target(ConnectorId::new("telegram")),
            SubscribeOptions::default(),
        )
        .await
        .unwrap();
    fixture
        .host
        .clone()
        .handle(
            message("telegram", "7", "7", "finish this task", false, true),
            &fixture.ctx,
        )
        .await;
    let original = next(&mut events, "chat.typing").await;
    fixture.request_count(1).await;
    let b = fixture.service.create_private(&a, "B").await.unwrap();
    let detached = fixture.service.presentations().event_for(&a, &b).unwrap();
    fixture
        .host
        .detach_presentation(&detached, &fixture.ctx)
        .await;
    next(&mut events, TURN_END).await;
    let back = fixture
        .service
        .open_private(&b, a.session_id(), b.selector_revision().unwrap())
        .await
        .unwrap();
    opened(&fixture, &back).await;
    let current = next(&mut events, "chat.typing").await;
    assert_ne!(token(&original).id(), token(&current).id());
    let final_reply = next(&mut events, "chat.reply").await;
    assert!(!token(&current).is_open());
    assert_eq!(
        final_reply
            .context
            .get::<SessionContext>()
            .unwrap()
            .selector_revision(),
        a.selector_revision()
    );
    assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
    assert!(fixture
        .host
        .history
        .load(&a.root_history_key())
        .await
        .iter()
        .any(|turn| turn.content.contains("| Main | preserved |")));
    opened(&fixture, &back).await;
    assert!(!token(&current).is_open());
}
