use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use chrono::Utc;
use octo_approval::Ingress;
use octo_core::{
    ChannelId, CogitatorContext, ConnectorCapabilities, ConnectorId, ConnectorInfo, Envelope,
    EventBus, EventKind, Filter, SubscribeOptions,
};
use octo_history::SqliteHistory;
use rig::tool::Tool;
use serde_json::{from_value, json, Value};
use tempfile::{tempdir, TempDir};
use tokio::{
    spawn,
    task::JoinHandle,
    time::{sleep, timeout},
};

use super::tools::SessionMessagingTool;
use crate::{
    cogitator::{
        provider_fixture::{setup, Server},
        AlbertCogitator, UserInput,
    },
    messaging::{
        contract::{ExecutionLocation, MessageId, TrustedSource, WorkRef},
        store::{MessageStatus, WorkState},
        transport::AdapterKind,
    },
    service::session::SessionService,
    sessions::{
        settings::{IdentityLink, SessionSettings},
        Admission, AdmissionScope, ExternalIdentity, ReplyRoute, SessionContext,
    },
};

pub(super) struct Fixture {
    pub host: Arc<AlbertCogitator>,
    pub ctx: CogitatorContext,
    pub service: Arc<SessionService>,
    pub server: Server,
    pub a: SessionContext,
    pub b: SessionContext,
    pub hits: Arc<AtomicUsize>,
    pub connector: JoinHandle<()>,
    scheduler: JoinHandle<()>,
    _directory: TempDir,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.connector.abort();
        self.scheduler.abort();
    }
}
impl Fixture {
    pub async fn new(model: &str) -> Self {
        let (mut host, base, server) = setup(model).await;
        let directory = tempdir().unwrap();
        Arc::get_mut(&mut host).unwrap().config.code_workspace = directory.path().into();
        let history = Arc::new(
            SqliteHistory::open_retained(directory.path().join("history.db"))
                .await
                .unwrap(),
        );
        let service = Arc::new(
            SessionService::from_history(
                history,
                SessionSettings {
                    identity_links: vec![IdentityLink {
                        connector: "telegram".into(),
                        actor: "7".into(),
                        user_id: "alice".into(),
                    }],
                    ..SessionSettings::default()
                },
            )
            .await
            .unwrap(),
        );
        let a = service
            .admit(
                Admission::new(
                    ExternalIdentity::new("telegram", "7").unwrap(),
                    ReplyRoute::new("telegram", "7", None).unwrap(),
                    AdmissionScope::Private,
                    "host",
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let b = service.create_private(&a, "Working session").await.unwrap();
        service
            .delivery_adapters()
            .register("telegram", AdapterKind::Telegram);
        host = host.with_sessions(service.clone());
        let ctx = CogitatorContext::new(
            base.shutdown.clone(),
            base.bus(),
            vec![
                ConnectorInfo {
                    id: ConnectorId::new("telegram"),
                    capabilities: ConnectorCapabilities::bidirectional()
                        .with_emit_kinds([EventKind::new("chat.message")])
                        .with_accept_kinds([EventKind::new("chat.reply")]),
                },
                ConnectorInfo {
                    id: ConnectorId::new("search"),
                    capabilities: ConnectorCapabilities::bidirectional()
                        .with_accept_kinds([EventKind::new("search.web")])
                        .with_description("search.web {query}: selected evidence"),
                },
                ConnectorInfo {
                    id: ConnectorId::new("sql"),
                    capabilities: ConnectorCapabilities::bidirectional()
                        .with_accept_kinds([EventKind::new("sql.query")])
                        .with_description("protected SQL"),
                },
            ],
        );
        let hits = Arc::new(AtomicUsize::new(0));
        let observed = hits.clone();
        let bus = ctx.bus();
        let mut requests = bus
            .subscribe(
                Filter::by_target(ConnectorId::new("search")),
                SubscribeOptions::default(),
            )
            .await
            .unwrap();
        let connector = spawn(async move {
            while let Some(request) = requests.next().await {
                assert_eq!(request.kind.as_str(), "search.web");
                assert!(request.context.get::<Ingress>().is_none());
                let placement = request.context.get::<ExecutionLocation>().unwrap();
                assert_eq!(placement.connector.as_deref(), Some("search"));
                observed.fetch_add(1, Ordering::SeqCst);
                bus.publish(
                    Envelope::new(
                        ConnectorId::new("search"),
                        EventKind::new("search.web.result"),
                        json!({"evidence":"completed-once-unique-evidence"}),
                    )
                    .with_correlation(request.id),
                )
                .await
                .unwrap();
            }
        });
        let mut alarms = ctx
            .bus()
            .subscribe(
                Filter::by_kind("octo.scheduler.list_alarms"),
                SubscribeOptions::default(),
            )
            .await
            .unwrap();
        let scheduler_bus = ctx.bus();
        let scheduler = spawn(async move {
            while let Some(request) = alarms.next().await {
                scheduler_bus
                    .publish(
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
            server,
            a,
            b,
            hits,
            connector,
            scheduler,
            _directory: directory,
        }
    }
    pub fn source(&self, context: &SessionContext) -> TrustedSource {
        TrustedSource::current(
            context,
            WorkRef::Turn("native-existing-work".into()),
            ExecutionLocation::default(),
        )
        .unwrap()
    }
    pub fn tool(&self) -> SessionMessagingTool {
        SessionMessagingTool::current(
            &self.host,
            &self.ctx,
            &self.a,
            WorkRef::Turn("native-existing-work".into()),
        )
        .unwrap()
    }
    pub async fn action(&self, action: Value) -> Value {
        self.tool().call(from_value(action).unwrap()).await.unwrap()
    }
    pub async fn question(&self, content: &str) -> MessageId {
        let sent = self.action(json!({"action":"send","destination":{"kind":"fixed_session","session":self.b.session_id()},"intent":"question","content":content})).await;
        assert_eq!(sent["accepted"], true, "{sent}");
        from_value(sent["message_id"].clone()).unwrap()
    }
    pub async fn status(&self, id: &MessageId) -> MessageStatus {
        self.host
            .messaging
            .as_ref()
            .unwrap()
            .store
            .status(&self.source(&self.a), id)
            .await
            .unwrap()
    }
    pub async fn wait_terminal(&self, id: &MessageId) -> MessageStatus {
        timeout(Duration::from_secs(4), async {
            loop {
                let status = self.status(id).await;
                if status
                    .work
                    .as_ref()
                    .is_some_and(|state| !matches!(state, WorkState::Queued | WorkState::Running))
                {
                    return status;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap()
    }
    pub async fn wait_requests(&self, count: usize) {
        timeout(Duration::from_secs(3), async {
            while self.server.requests.lock().unwrap().len() < count {
                sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
    }
    pub async fn native(&self, context: &SessionContext, text: &str) {
        let mut envelope = Envelope::new(
            ConnectorId::new("telegram"),
            EventKind::new("chat.message"),
            text.to_owned(),
        )
        .with_channel(ChannelId::new("7"));
        envelope.context.insert(context.clone());
        self.host
            .clone()
            .spawn_turn(
                Arc::new(envelope),
                UserInput {
                    text: text.into(),
                    images: vec![],
                    seed: None,
                    voice: None,
                },
                &self.ctx,
            )
            .await;
    }
    pub fn now(&self) -> i64 {
        Utc::now().timestamp_millis()
    }
}
