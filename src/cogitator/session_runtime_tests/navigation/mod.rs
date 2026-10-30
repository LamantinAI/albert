use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use octo_connector_telegram::{TurnFeedback, TURN_END};
use octo_core::{
    CogitatorContext, ConnectorCapabilities, ConnectorId, ConnectorInfo, Envelope, EventBus,
    EventKind, Filter, SubscribeOptions, Subscription,
};
use serde_json::json;
use tokio::{spawn, time::timeout};

use super::{message, Fixture};
use crate::{
    cogitator::{compaction::CompactOptions, UserInput},
    history::{HistoryStore, Turn},
    sessions::SessionContext,
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

#[tokio::test]
async fn detach_ends_exact_presentation_but_tools_journal_and_pinned_final_continue() {
    let fixture = Fixture::with_model("nav-feedback-tools").await;
    let mut connectors = fixture.ctx.connectors().to_vec();
    connectors.push(ConnectorInfo {
        id: ConnectorId::new("search"),
        capabilities: ConnectorCapabilities::bidirectional()
            .with_accept_kinds([EventKind::new("search.web")])
            .with_description("search.web {query}"),
    });
    let ctx = CogitatorContext::new(fixture.ctx.shutdown.clone(), fixture.ctx.bus(), connectors);
    let mut work = ctx
        .bus()
        .subscribe(
            Filter::by_target(ConnectorId::new("search")),
            SubscribeOptions::default(),
        )
        .await
        .unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let counted = count.clone();
    let bus = ctx.bus();
    let connector = spawn(async move {
        while let Some(request) = work.next().await {
            counted.fetch_add(1, Ordering::SeqCst);
            bus.publish(
                Envelope::new(
                    ConnectorId::new("search"),
                    EventKind::new("search.web.result"),
                    json!({"evidence":"NAV-TOOL-EVIDENCE"}),
                )
                .with_correlation(request.id),
            )
            .await
            .unwrap();
        }
    });
    let original = fixture.context("telegram", "7", "7").await;
    let mut telegram = ctx
        .bus()
        .subscribe(
            Filter::by_target(ConnectorId::new("telegram")),
            SubscribeOptions::default(),
        )
        .await
        .unwrap();
    let mut console = ctx
        .bus()
        .subscribe(
            Filter::by_target(ConnectorId::new("console")),
            SubscribeOptions::default(),
        )
        .await
        .unwrap();
    let mut cancel = ctx
        .bus()
        .subscribe(
            Filter::by_kind("octo.control.cancel"),
            SubscribeOptions::default(),
        )
        .await
        .unwrap();
    fixture
        .host
        .clone()
        .handle(
            message("telegram", "7", "7", "continue tools", false, true),
            &ctx,
        )
        .await;
    let typing = next(&mut telegram, "chat.typing").await;
    fixture.request_count(1).await;
    fixture
        .host
        .clone()
        .handle(
            message("console", "local", "stdin", "independent work", false, true),
            &ctx,
        )
        .await;
    let other = next(&mut console, "chat.typing").await;
    let selected = fixture
        .service
        .create_private(&original, "Next")
        .await
        .unwrap();
    let changed = fixture
        .service
        .presentations()
        .event_for(&original, &selected)
        .unwrap();
    let latest = fixture
        .service
        .create_private(&selected, "Third view")
        .await
        .unwrap();
    let newer = fixture
        .service
        .presentations()
        .event_for(&selected, &latest)
        .unwrap();
    fixture.host.detach_presentation(&newer, &ctx).await;
    assert!(token(&typing).is_open());
    fixture
        .host
        .clone()
        .handle(
            Arc::new(Envelope::new(
                ConnectorId::new("albert.sessions"),
                EventKind::new("albert.presentation.detached"),
                json!({"claimed":"detach"}),
            )),
            &ctx,
        )
        .await;
    assert!(token(&typing).is_open());
    fixture
        .host
        .clone()
        .handle(
            Arc::new(
                Envelope::new(
                    ConnectorId::new("albert.sessions"),
                    EventKind::new("albert.presentation.detached"),
                    json!({}),
                )
                .with_context(changed),
            ),
            &ctx,
        )
        .await;
    let ended = next(&mut telegram, TURN_END).await;
    assert_eq!(token(&ended).id(), token(&typing).id());
    assert!(!token(&typing).is_open());
    assert!(token(&other).is_open());
    assert!(original.is_live());
    assert!(timeout(Duration::from_millis(30), cancel.next())
        .await
        .is_err());
    let mut late = ctx
        .bus()
        .subscribe(
            Filter::by_kind("chat.status").with_target(ConnectorId::new("telegram")),
            SubscribeOptions::default(),
        )
        .await
        .unwrap();
    let feed = StatusFeed::new(
        ctx.bus(),
        ConnectorId::new("albert"),
        ConnectorId::new("telegram"),
        typing.channel.clone(),
    )
    .with_feedback(Some(token(&typing)));
    feed.progress("late progress must not reopen old bubble")
        .await;
    assert!(timeout(Duration::from_millis(30), late.next())
        .await
        .is_err());
    let reply = next(&mut telegram, "chat.reply").await;
    assert!(reply
        .payload_as::<String>()
        .unwrap()
        .contains("answer from nav-feedback-tools"));
    assert_eq!(
        reply.context.get::<SessionContext>().unwrap().session_id(),
        original.session_id()
    );
    assert_eq!(reply.channel, typing.channel);
    assert_eq!(count.load(Ordering::SeqCst), 1);
    let records = fixture
        .router
        .strict_load(&original.root_history_key())
        .await
        .unwrap();
    assert!(records
        .iter()
        .any(|record| record.content.contains("NAV-TOOL-EVIDENCE")));
    assert!(records
        .iter()
        .any(|record| record.content.contains("answer from nav-feedback-tools")));
    assert!(fixture
        .router
        .strict_load(&selected.root_history_key())
        .await
        .unwrap()
        .is_empty());
    assert!(fixture
        .router
        .strict_load(&latest.root_history_key())
        .await
        .unwrap()
        .is_empty());
    fixture.host.stop_turns(&ctx).await;
    connector.abort();
}

#[tokio::test]
async fn monotonic_cutoffs_survive_out_of_order_events_and_new_generations() {
    let fixture = Fixture::new().await;
    let a = fixture.context("telegram", "7", "7").await;
    let precreated = fixture.host.fresh_feedback(
        &Arc::new(
            Envelope::new(
                ConnectorId::new("telegram"),
                EventKind::new("chat.message"),
                "captured".to_owned(),
            )
            .with_context(a.clone()),
        ),
        "created-before-detach",
    );
    assert!(token(&precreated).is_open());
    let b = fixture.service.create_private(&a, "B").await.unwrap();
    let ab = fixture.service.presentations().event_for(&a, &b).unwrap();
    let c = fixture.service.create_private(&b, "C").await.unwrap();
    let bc = fixture.service.presentations().event_for(&b, &c).unwrap();
    assert!(ab.matches(&a));
    let other_route = fixture.context("console", "local", "stdin").await;
    assert!(!ab.matches(&other_route));
    assert!(!fixture.service.presentations().detached(&other_route));
    assert!(bc.matches(&b));
    assert!(fixture.service.presentations().detached(&a));
    assert!(fixture.service.presentations().detached(&b));
    fixture.host.fence_feedback(&precreated);
    assert!(!token(&precreated).is_open());
    let back = fixture
        .service
        .open_private(&c, a.session_id(), c.selector_revision().unwrap())
        .await
        .unwrap();
    assert!(!ab.matches(&back));
    assert!(!fixture.service.presentations().detached(&back));
    let incoming = Arc::new(
        Envelope::new(
            ConnectorId::new("telegram"),
            EventKind::new("chat.message"),
            "new generation".to_owned(),
        )
        .with_context(back.clone()),
    );
    let fresh = fixture.host.fresh_feedback(&incoming, "fresh-a");
    assert!(token(&fresh).is_open());
    fixture.host.detach_presentation(&bc, &fixture.ctx).await;
    fixture.host.detach_presentation(&ab, &fixture.ctx).await;
    assert!(token(&fresh).is_open());
    let old = Arc::new(
        Envelope::new(
            ConnectorId::new("telegram"),
            EventKind::new("chat.message"),
            "accepted before departure".to_owned(),
        )
        .with_context(a),
    );
    assert!(!token(&fixture.host.fresh_feedback(&old, "late-native")).is_open());
    assert!(!token(&fixture.host.fresh_feedback(&old, "late-compact")).is_open());
    assert!(!token(&fixture.host.fresh_feedback(&old, "late-resume")).is_open());
    fixture
        .host
        .clone()
        .spawn_turn(
            incoming.clone(),
            UserInput {
                text: "new generation".into(),
                images: Vec::new(),
                seed: None,
                voice: None,
            },
            &fixture.ctx,
        )
        .await;
    let state = fixture
        .host
        .turns
        .lock()
        .unwrap()
        .get(&("session".into(), back.root_history_key()))
        .unwrap()
        .clone();
    let registered = {
        let gate = state.lock().await;
        token(gate.feedback().unwrap())
    };
    assert!(registered.is_open());
    fixture.host.detach_presentation(&ab, &fixture.ctx).await;
    assert!(registered.is_open());
    fixture.host.end_feedback(&fresh, &fixture.ctx).await;
    assert!(token(&fixture.host.fresh_feedback(&incoming, "next-same-view")).is_open());
    fixture.host.stop_turns(&fixture.ctx).await;
}

#[tokio::test]
async fn same_session_open_and_save_do_not_detach_and_registry_grows_only_by_route() {
    let fixture = Fixture::new().await;
    let a = fixture.context("telegram", "7", "7").await;
    let same = fixture
        .service
        .open_private(&a, a.session_id(), a.selector_revision().unwrap())
        .await
        .unwrap();
    assert!(!fixture.service.presentations().detached(&a));
    assert!(fixture
        .service
        .presentations()
        .event_for(&a, &same)
        .is_none());
    let tmp = fixture.service.start_temporary(&same, None).await.unwrap();
    let repeated = fixture.service.start_temporary(&tmp, None).await.unwrap();
    assert!(fixture
        .service
        .presentations()
        .event_for(&tmp, &repeated)
        .is_none());
    let saved = fixture
        .service
        .save_temporary(&tmp, "presentation-save", None)
        .await
        .unwrap();
    assert!(!fixture.service.presentations().detached(&tmp));
    assert!(fixture
        .service
        .presentations()
        .event_for(&tmp, &saved)
        .is_none());
    let ended = fixture.service.end_temporary(&saved, None).await.unwrap();
    assert!(saved.is_live());
    assert!(fixture.service.presentations().detached(&saved));
    let mut current = ended.context;
    for _ in 0..20 {
        current = fixture
            .service
            .create_private(&current, "Next")
            .await
            .unwrap();
    }
    assert_eq!(fixture.service.presentations().route_count(), 1);
    let console = fixture.context("console", "local", "stdin").await;
    let _ = fixture
        .service
        .open_private(
            &console,
            current.session_id(),
            console.selector_revision().unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(fixture.service.presentations().route_count(), 2);
}

#[tokio::test]
async fn detached_compact_and_resume_tokens_cannot_reopen_old_view() {
    let fixture = Fixture::with_model("feedback-phases").await;
    let original = fixture.context("telegram", "7", "7").await;
    fixture
        .router
        .append(
            &original.root_history_key(),
            &[Turn::user("old question"), Turn::assistant("old answer")],
        )
        .await
        .unwrap();
    let mut output = fixture
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
            message("telegram", "7", "7", "continue old work", false, true),
            &fixture.ctx,
        )
        .await;
    let initial = next(&mut output, "chat.typing").await;
    fixture.request_count(1).await;
    let selected = fixture
        .service
        .create_private(&original, "New view")
        .await
        .unwrap();
    let change = fixture
        .service
        .presentations()
        .event_for(&original, &selected)
        .unwrap();
    fixture
        .host
        .detach_presentation(&change, &fixture.ctx)
        .await;
    assert!(!token(&initial).is_open());
    let compact = Arc::new(
        Envelope::new(
            ConnectorId::new("telegram"),
            EventKind::new("chat.message"),
            "/compact".to_owned(),
        )
        .with_context(original.clone()),
    );
    fixture
        .host
        .clone()
        .spawn_compact(compact, &fixture.ctx, CompactOptions::default())
        .await;
    let state = fixture
        .host
        .turns
        .lock()
        .unwrap()
        .get(&("session".into(), original.root_history_key()))
        .unwrap()
        .clone();
    {
        let gate = state.lock().await;
        assert!(!token(gate.feedback().unwrap()).is_open());
    }
    let final_reply = next(&mut output, "chat.reply").await;
    assert!(!token(&final_reply).is_open());
    assert_eq!(
        final_reply
            .context
            .get::<SessionContext>()
            .unwrap()
            .session_id(),
        original.session_id()
    );
    assert!(original.is_live());
    assert!(fixture
        .router
        .strict_load(&original.root_history_key())
        .await
        .unwrap()
        .iter()
        .any(|record| record.content.contains("answer from feedback-phases")));
    fixture.host.stop_turns(&fixture.ctx).await;
}

#[tokio::test]
async fn accepted_old_view_registered_after_detach_cannot_reopen_presentation() {
    let fixture = Fixture::new().await;
    let original = fixture.context("telegram", "7", "7").await;
    let captured = Arc::new(
        Envelope::new(
            ConnectorId::new("telegram"),
            EventKind::new("chat.message"),
            "captured old input".to_owned(),
        )
        .with_context(original.clone()),
    );
    let selected = fixture
        .service
        .create_private(&original, "New current view")
        .await
        .unwrap();
    let change = fixture
        .service
        .presentations()
        .event_for(&original, &selected)
        .unwrap();
    fixture
        .host
        .detach_presentation(&change, &fixture.ctx)
        .await;
    let mut replies = fixture.replies().await;
    fixture
        .host
        .clone()
        .spawn_turn(
            captured,
            UserInput {
                text: "captured old input".into(),
                images: Vec::new(),
                seed: None,
                voice: None,
            },
            &fixture.ctx,
        )
        .await;
    let state = fixture
        .host
        .turns
        .lock()
        .unwrap()
        .get(&("session".into(), original.root_history_key()))
        .unwrap()
        .clone();
    {
        let gate = state.lock().await;
        assert!(!token(gate.feedback().unwrap()).is_open());
    }
    let reply = next(&mut replies, "chat.reply").await;
    assert_eq!(
        reply.context.get::<SessionContext>().unwrap().session_id(),
        original.session_id()
    );
    assert!(!token(&reply).is_open());
    let records = fixture
        .router
        .strict_load(&original.root_history_key())
        .await
        .unwrap();
    assert!(records
        .iter()
        .any(|record| record.content.contains("captured old input")));
    assert!(records
        .iter()
        .any(|record| record.content.contains("answer from session-delayed")));
    assert!(fixture
        .router
        .strict_load(&selected.root_history_key())
        .await
        .unwrap()
        .is_empty());
    fixture.host.stop_turns(&fixture.ctx).await;
}
