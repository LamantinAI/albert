use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};

use octo_core::{
    ChannelId, ChannelMetadata, CogitatorContext, ConnectorCapabilities, ConnectorId,
    ConnectorInfo, Envelope, EventBus, EventKind, Filter, SubscribeOptions, Subscription,
};
use octo_history::SqliteHistory;
use octo_rig::DispatchArgs;
use serde_json::json;
use tempfile::{tempdir, TempDir};
use tokio::{
    spawn,
    task::JoinHandle,
    time::{sleep, timeout},
};

use super::{
    provider_fixture::{setup, Server},
    session_dispatch::pin_alarm,
    AlbertCogitator,
};
use crate::{
    history::sessions::SessionHistory,
    service::session::SessionService,
    sessions::{
        settings::{IdentityLink, SessionSettings},
        Admission, AdmissionScope, ExternalIdentity, ReplyRoute, SessionContext,
    },
};

struct Fixture {
    host: Arc<AlbertCogitator>,
    ctx: CogitatorContext,
    service: Arc<SessionService>,
    router: Arc<SessionHistory>,
    server: Server,
    scheduler: JoinHandle<()>,
    _directory: TempDir,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.scheduler.abort();
    }
}

impl Fixture {
    async fn new() -> Self {
        Self::with_model("session-delayed").await
    }

    async fn with_model(model: &str) -> Self {
        let (mut host, ctx, server) = setup(model).await;
        let directory = tempdir().unwrap();
        Arc::get_mut(&mut host).unwrap().config.code_workspace = directory.path().into();
        let history = Arc::new(
            SqliteHistory::open_retained(directory.path().join("history.db"))
                .await
                .unwrap(),
        );
        let service = Arc::new(
            SessionService::from_history(
                history.clone(),
                SessionSettings {
                    identity_links: vec![
                        IdentityLink {
                            connector: "telegram".into(),
                            actor: "7".into(),
                            user_id: "test-owner".into(),
                        },
                        IdentityLink {
                            connector: "console".into(),
                            actor: "local".into(),
                            user_id: "test-owner".into(),
                        },
                    ],
                    ..SessionSettings::default()
                },
            )
            .await
            .unwrap(),
        );
        let router = service.journal_router().clone();
        host = host.with_sessions(service.clone());
        let connectors = ["telegram", "console"]
            .into_iter()
            .map(|id| ConnectorInfo {
                id: ConnectorId::new(id),
                capabilities: ConnectorCapabilities {
                    event_kinds_emit: vec![EventKind::new("chat.message")],
                    ..Default::default()
                },
            })
            .collect();
        let ctx = CogitatorContext::new(ctx.shutdown.clone(), ctx.bus(), connectors);
        let mut requests = ctx
            .bus()
            .subscribe(
                Filter::by_kind("octo.scheduler.list_alarms"),
                SubscribeOptions::default(),
            )
            .await
            .unwrap();
        let bus = ctx.bus();
        let scheduler = spawn(async move {
            while let Some(request) = requests.next().await {
                bus.publish(
                    Envelope::new(
                        ConnectorId::new("scheduler"),
                        EventKind::new("octo.scheduler.list_alarms.result"),
                        json!({"alarms":[]}),
                    )
                    .with_correlation(request.id),
                )
                .await
                .unwrap();
            }
        });
        Self {
            host,
            ctx,
            service,
            router,
            server,
            scheduler,
            _directory: directory,
        }
    }

    async fn replies(&self) -> Subscription {
        self.ctx
            .bus()
            .subscribe(Filter::by_kind("chat.reply"), SubscribeOptions::default())
            .await
            .unwrap()
    }

    async fn context(&self, connector: &str, actor: &str, channel: &str) -> SessionContext {
        self.service
            .admit(
                Admission::new(
                    ExternalIdentity::new(connector, actor).unwrap(),
                    ReplyRoute::new(connector, channel, None).unwrap(),
                    AdmissionScope::Private,
                    "host",
                )
                .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn request_count(&self, count: usize) {
        timeout(Duration::from_secs(3), async {
            loop {
                if self.server.requests.lock().unwrap().len() >= count {
                    break;
                }
                sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
    }
}

fn message(
    connector: &str,
    actor: &str,
    channel: &str,
    text: &str,
    group: bool,
    owner: bool,
) -> Arc<Envelope> {
    let metadata = ChannelMetadata::default()
        .with_tag("sender_id", actor)
        .with_tag("chat_type", if group { "group" } else { "private" })
        .with_tag("addressed", "true")
        .with_tag("role", if owner { "owner" } else { "trusted" });
    Arc::new(
        Envelope::new(
            ConnectorId::new(connector),
            EventKind::new("chat.message"),
            text.to_owned(),
        )
        .with_channel(ChannelId::new(channel))
        .with_channel_metadata(metadata),
    )
}

async fn reply(replies: &mut Subscription) -> Arc<Envelope> {
    timeout(Duration::from_secs(4), replies.next())
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn linked_transports_share_gate_and_retry_does_not_interrupt_or_append() {
    let fixture = Fixture::new().await;
    let mut replies = fixture.replies().await;
    let first = message("telegram", "7", "7", "first private input", false, true);
    fixture
        .host
        .clone()
        .handle(first.clone(), &fixture.ctx)
        .await;
    fixture.request_count(1).await;
    let original = fixture.context("telegram", "7", "7").await;
    let console = fixture.context("console", "local", "console").await;
    fixture
        .service
        .open_private(
            &console,
            original.session_id(),
            console.selector_revision().unwrap(),
        )
        .await
        .unwrap();
    let seq = fixture.host.turn_seq.load(Ordering::Relaxed);
    fixture.host.clone().handle(first, &fixture.ctx).await;
    assert_eq!(fixture.host.turn_seq.load(Ordering::Relaxed), seq);
    let second = message(
        "console",
        "local",
        "console",
        "second private input",
        false,
        true,
    );
    fixture
        .host
        .clone()
        .handle(second.clone(), &fixture.ctx)
        .await;
    fixture.request_count(2).await;
    let answer = reply(&mut replies).await;
    assert_eq!(answer.correlation_id, Some(second.id));
    assert_eq!(answer.target.as_ref().unwrap().as_str(), "console");
    assert_eq!(answer.channel.as_ref().unwrap().as_str(), "console");
    assert_eq!(fixture.host.turns.lock().unwrap().len(), 1);
    let stored = fixture
        .router
        .strict_load(&original.root_history_key())
        .await
        .unwrap();
    assert_eq!(
        stored
            .iter()
            .filter(|turn| turn.content.contains("first private input"))
            .count(),
        1
    );
    assert_eq!(
        stored
            .iter()
            .filter(|turn| turn.content.contains("second private input"))
            .count(),
        1
    );
    let requests = fixture.server.requests.lock().unwrap();
    assert!(requests[1]["messages"]
        .to_string()
        .contains("first private input"));
    drop(requests);
    fixture.host.stop_turns(&fixture.ctx).await;
}

#[tokio::test]
async fn different_selected_sessions_keep_late_replies_and_contexts_separate() {
    let fixture = Fixture::new().await;
    let mut replies = fixture.replies().await;
    let first = message("telegram", "7", "7", "first-session-secret", false, true);
    fixture
        .host
        .clone()
        .handle(first.clone(), &fixture.ctx)
        .await;
    fixture.request_count(1).await;
    let original = fixture.context("telegram", "7", "7").await;
    let selected = fixture
        .service
        .create_private(&original, "Second")
        .await
        .unwrap();
    let second = message("telegram", "7", "7", "second-session-question", false, true);
    fixture
        .host
        .clone()
        .handle(second.clone(), &fixture.ctx)
        .await;
    fixture.request_count(2).await;
    let a = reply(&mut replies).await;
    let b = reply(&mut replies).await;
    assert!(a.correlation_id == Some(first.id) || b.correlation_id == Some(first.id));
    assert!(a.correlation_id == Some(second.id) || b.correlation_id == Some(second.id));
    assert_eq!(fixture.host.turns.lock().unwrap().len(), 2);
    let requests = fixture.server.requests.lock().unwrap();
    assert!(!requests[1]["messages"]
        .to_string()
        .contains("first-session-secret"));
    drop(requests);
    assert_eq!(
        fixture
            .router
            .strict_load(&original.root_history_key())
            .await
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        fixture
            .router
            .strict_load(&selected.root_history_key())
            .await
            .unwrap()
            .len(),
        2
    );
    fixture.host.stop_turns(&fixture.ctx).await;
}

#[tokio::test]
async fn group_chatter_is_quiet_and_owners_do_not_transfer_authority_to_other_actors() {
    let fixture = Fixture::new().await;
    let mut replies = fixture.replies().await;
    let first = message("telegram", "7", "group", "owner group task", true, true);
    fixture
        .host
        .clone()
        .handle(first.clone(), &fixture.ctx)
        .await;
    fixture.request_count(1).await;
    let seq = fixture.host.turn_seq.load(Ordering::Relaxed);
    let mut quiet = (*message("telegram", "8", "group", "quiet chatter", true, false)).clone();
    quiet
        .channel_metadata
        .as_mut()
        .unwrap()
        .tags
        .insert("addressed".into(), "false".into());
    fixture
        .host
        .clone()
        .handle(Arc::new(quiet), &fixture.ctx)
        .await;
    assert_eq!(fixture.host.turn_seq.load(Ordering::Relaxed), seq);
    let next = message("telegram", "8", "group", "ordinary group task", true, false);
    fixture
        .host
        .clone()
        .handle(next.clone(), &fixture.ctx)
        .await;
    fixture.request_count(2).await;
    let answer = reply(&mut replies).await;
    assert_eq!(answer.correlation_id, Some(next.id));
    let wire = fixture.server.requests.lock().unwrap()[1]["tools"].to_string();
    assert!(!wire.contains("config_set_secret"));
    assert!(!wire.contains("restart"));
    assert_eq!(fixture.host.turns.lock().unwrap().len(), 1);
    fixture.host.stop_turns(&fixture.ctx).await;
}

#[tokio::test]
async fn scheduler_payload_is_host_pinned_and_legacy_alarm_uses_original_default() {
    let fixture = Fixture::new().await;
    let original = fixture.context("telegram", "7", "7").await;
    let mut args = DispatchArgs {
        target: "scheduler".into(),
        kind: "octo.scheduler.add_alarm".into(),
        channel: None,
        payload: json!({"payload":{"task":"check experiment","channel":"attacker","reply_via":"other","albert_session_id":"forged","routine":"memory_reflection"}}),
    };
    pin_alarm(&mut args, Some(&original)).unwrap();
    assert_eq!(
        args.payload["payload"]["albert_session_id"],
        original.session_id().as_str()
    );
    assert_eq!(args.payload["payload"]["channel"], "7");
    assert!(args.payload["payload"].get("routine").is_none());
    let selected = fixture
        .service
        .create_private(&original, "Other work")
        .await
        .unwrap();
    let mut replies = fixture.replies().await;
    let fired = Arc::new(
        Envelope::new(
            ConnectorId::new("scheduler"),
            EventKind::new("alarm.fired"),
            json!({"task":"original task","channel":"7","reply_via":"telegram"}),
        )
        .with_tag("alarm_id", "legacy-alarm"),
    );
    fixture.host.on_alarm(fired.clone(), &fixture.ctx).await;
    let answer = reply(&mut replies).await;
    assert_eq!(answer.channel.as_ref().unwrap().as_str(), "7");
    assert_eq!(answer.target.as_ref().unwrap().as_str(), "telegram");
    assert_eq!(answer.correlation_id, Some(fired.id));
    assert_eq!(
        fixture
            .router
            .strict_load(&original.root_history_key())
            .await
            .unwrap()
            .len(),
        2
    );
    assert!(fixture
        .router
        .strict_load(&selected.root_history_key())
        .await
        .unwrap()
        .is_empty());
    fixture.host.stop_turns(&fixture.ctx).await;
}

mod child;
mod compaction;
mod feedback;
mod migration;
mod native;
mod navigation;
mod picker;
mod reattach;
mod replay;
mod temporary;
mod titles;
