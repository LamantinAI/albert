use std::{sync::Arc, time::Duration};

use octo_core::{
    ChannelId, ChannelMetadata, CogitatorContext, ConnectorCapabilities, ConnectorId,
    ConnectorInfo, Envelope, EventBus, EventKind, Filter, SubscribeOptions, Subscription,
};
use octo_history::SqliteHistory;
use serde_json::json;
use tempfile::{tempdir, TempDir};
use tokio::{spawn, task::JoinHandle, time::sleep, time::timeout};

use crate::{
    cogitator::provider_fixture::setup, cogitator::provider_fixture::Server,
    cogitator::AlbertCogitator, history::sessions::SessionHistory,
    service::session::SessionService, sessions::settings::IdentityLink,
    sessions::settings::SessionSettings, sessions::Admission, sessions::AdmissionScope,
    sessions::ExternalIdentity, sessions::ReplyRoute, sessions::SessionContext,
};

pub struct Fixture {
    pub host: Arc<AlbertCogitator>,
    pub ctx: CogitatorContext,
    pub service: Arc<SessionService>,
    pub router: Arc<SessionHistory>,
    pub server: Server,
    scheduler: JoinHandle<()>,
    pub(crate) _directory: TempDir,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.scheduler.abort();
    }
}

impl Fixture {
    pub async fn new() -> Self {
        Self::with_model("session-delayed").await
    }

    pub async fn with_model(model: &str) -> Self {
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

    pub async fn replies(&self) -> Subscription {
        self.ctx
            .bus()
            .subscribe(Filter::by_kind("chat.reply"), SubscribeOptions::default())
            .await
            .unwrap()
    }

    pub async fn context(&self, connector: &str, actor: &str, channel: &str) -> SessionContext {
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

    pub async fn request_count(&self, count: usize) {
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

pub fn message(
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

pub async fn reply(replies: &mut Subscription) -> Arc<Envelope> {
    timeout(Duration::from_secs(4), replies.next())
        .await
        .unwrap()
        .unwrap()
}
