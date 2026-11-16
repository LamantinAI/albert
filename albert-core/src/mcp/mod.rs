//! Albert's actor policy and provider-native bridge over the standalone Octo client.
mod config;
mod lifecycle;
mod tool;

pub use config::{valid_id, RegistryConfig, ServerSpec};
pub use tool::McpTool;

use std::{
    collections::BTreeMap, mem::take, path::PathBuf, sync::atomic::AtomicU64, sync::Arc,
    sync::RwLock,
};

use octo_core::control::CancellationToken;
use octo_mcp::{ClientLimits, Credentials, McpError, ServerSession, ToolSnapshot};
use serde::Serialize;
use thiserror::Error;
use tokio::sync::Mutex;

#[derive(Debug, Error)]
pub enum McpRegistryError {
    #[error("Invalid MCP registry configuration.")]
    Configuration,
    #[error("MCP tools require a live saved owner conversation with host permissions.")]
    Access,
    #[error("This MCP tool is unavailable or its access changed. Use a fresh turn after reviewing the connection.")]
    Stale,
    #[error("MCP tool names collide; review server identifiers before enabling them.")]
    Collision,
    #[error("MCP arguments must be a JSON object.")]
    Arguments,
    #[error(transparent)]
    Client(#[from] McpError),
}
pub type Result<T> = std::result::Result<T, McpRegistryError>;
#[derive(Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ServerPhase {
    Disabled,
    Connecting,
    Connected,
    Error,
}
#[derive(Clone, Serialize)]
pub struct ServerStatus {
    pub id: String,
    pub phase: ServerPhase,
    pub tool_count: usize,
    pub generation: u64,
    pub error: Option<String>,
}
struct Entry {
    spec: ServerSpec,
    generation: u64,
    limits: ClientLimits,
    client: Option<Arc<ServerSession>>,
    phase: ServerPhase,
    error: Option<String>,
    contract_valid: bool,
    cancel: CancellationToken,
    grants: BTreeMap<String, CancellationToken>,
}
pub struct McpRegistry {
    base: PathBuf,
    credentials: Arc<dyn Credentials>,
    entries: RwLock<BTreeMap<String, Entry>>,
    apply: Mutex<()>,
    generation: AtomicU64,
}
impl McpRegistry {
    pub fn new(base: PathBuf, credentials: Arc<dyn Credentials>) -> Arc<Self> {
        let base = base.canonicalize().unwrap_or(base);
        Arc::new(Self {
            base,
            credentials,
            entries: RwLock::new(BTreeMap::new()),
            apply: Mutex::new(()),
            generation: AtomicU64::new(1),
        })
    }
    pub fn status(&self, id: &str) -> Option<ServerStatus> {
        self.entries.read().unwrap().get(id).map(|entry| {
            let disconnected=entry.client.as_ref().is_some_and(|client|client.connected_now()==Some(false));
            ServerStatus{id:id.into(),phase:if disconnected && entry.phase==ServerPhase::Connected {ServerPhase::Error}else{entry.phase},tool_count:entry.client.as_ref().map(|client|client.snapshot().tools.len()).unwrap_or(0),generation:entry.generation,error:entry.error.clone().or_else(||disconnected.then(||"The connection is closed. A new call can reconnect without replaying earlier calls.".into()))}
        })
    }
    pub fn tools(&self, id: &str) -> Option<Arc<ToolSnapshot>> {
        self.entries
            .read()
            .unwrap()
            .get(id)
            .and_then(|entry| entry.client.as_ref().map(|client| client.snapshot()))
    }
    pub async fn close(&self) {
        let entries = take(&mut *self.entries.write().unwrap());
        for (_, entry) in entries {
            entry.cancel.cancel();
            if let Some(client) = entry.client {
                let _ = client.close().await;
            }
        }
    }
}
impl Drop for McpRegistry {
    fn drop(&mut self) {
        for entry in self.entries.get_mut().unwrap().values() {
            entry.cancel.cancel();
        }
    }
}

#[cfg(test)]
mod fixture;
#[cfg(test)]
mod tests {
    use super::{
        fixture::{server, Fake},
        tool::tool_name,
        McpRegistry, RegistryConfig, ServerPhase, ServerSpec,
    };
    use crate::{
        db::repo::identity::bind, service::session::web::WebActor,
        service::session::SessionService, sessions::actor::ActorToolPolicy,
        sessions::settings::SessionSettings, sessions::SessionContext, sessions::UserId,
    };
    use octo_history::SqliteHistory;
    use octo_mcp::{EnvironmentCredentials, ServerConfig};
    use rig::tool::ToolDyn;
    use sqlx::query;
    use std::{sync::Arc, time::Duration};
    use tempfile::{tempdir, TempDir};
    use tokio::time::{sleep, timeout};

    async fn actor() -> (TempDir, SessionService, SessionContext) {
        let dir = tempdir().unwrap();
        let history = Arc::new(
            SqliteHistory::open_retained(dir.path().join("history.db"))
                .await
                .unwrap(),
        );
        let service = SessionService::from_history(history, SessionSettings::default())
            .await
            .unwrap();
        let mut tx = service.history().pool().begin().await.unwrap();
        bind(&mut tx, "web", "account", "person").await.unwrap();
        tx.commit().await.unwrap();
        query("INSERT INTO albert_login_accounts(id,username,password_hash,role,canonical_user_id,must_change_password) VALUES ('account','fixture','unused','owner','person',0)").execute(service.database().pool()).await.unwrap();
        let actor = WebActor {
            account_id: "account".into(),
            user_id: UserId::parse("person").unwrap(),
            tools: ActorToolPolicy::ExistingHost,
        };
        let session = service
            .web_create(&actor, "MCP fixture", false)
            .await
            .unwrap();
        let context = service
            .web_context(&actor, &session.id)
            .await
            .unwrap()
            .with_account(service.accounts().account("account").await.unwrap());
        (dir, service, context)
    }
    async fn connected(registry: &McpRegistry) {
        timeout(Duration::from_secs(3), async {
            loop {
                if registry
                    .status("local")
                    .is_some_and(|state| state.phase == ServerPhase::Connected)
                {
                    break;
                }
                sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
    }
    fn config(url: String, tools: &[&str]) -> RegistryConfig {
        RegistryConfig {
            servers: vec![ServerSpec {
                id: "local".into(),
                name: "Local service".into(),
                description: String::new(),
                enabled: true,
                connection: ServerConfig::Http {
                    url,
                    token_env: None,
                },
                allowed_tools: tools.iter().map(|name| (*name).into()).collect(),
            }],
            ..Default::default()
        }
    }
    #[tokio::test]
    async fn native_snapshot_preserves_definitions_and_current_grants_without_exposing_new_tools_to_old_parents(
    ) {
        let (dir, service, actor) = actor().await;
        let fake = Fake::default();
        let (url, task) = server(fake.clone()).await;
        let registry = McpRegistry::new(dir.path().into(), Arc::new(EnvironmentCredentials));
        registry
            .configure(config(url.clone(), &["read"]))
            .await
            .unwrap();
        connected(&registry).await;
        let old = registry.snapshot(Some(&actor)).unwrap();
        assert_eq!(old.len(), 1);
        let definition = old[0].definition(String::new()).await;
        assert_eq!(definition.parameters["properties"]["version"]["const"], 0);
        assert!(old[0]
            .call("{}".into())
            .await
            .unwrap()
            .contains("structuredContent"));
        assert!(old[0].call("[]".into()).await.is_err());
        registry
            .configure(config(url.clone(), &["read", "write"]))
            .await
            .unwrap();
        assert_eq!(registry.snapshot(Some(&actor)).unwrap().len(), 2);
        assert_eq!(old.len(), 1);
        registry
            .configure(config(url.clone(), &["write"]))
            .await
            .unwrap();
        assert!(old[0].call("{}".into()).await.is_err());
        assert_eq!(fake.0.lock().unwrap().calls, vec!["read"]);
        registry
            .configure(config(url, &["read", "write"]))
            .await
            .unwrap();
        assert!(
            old[0].call("{}".into()).await.is_err(),
            "revoked handles must not revive when a tool is allowed again"
        );
        registry
            .snapshot(Some(&actor))
            .unwrap()
            .remove(0)
            .call("{}".into())
            .await
            .unwrap();
        assert!(registry.snapshot(None).unwrap().is_empty());
        let mut guest = actor.clone();
        guest.actor_tools = Some(ActorToolPolicy::NoTools);
        assert!(registry.snapshot(Some(&guest)).unwrap().is_empty());
        let write = registry.snapshot(Some(&actor)).unwrap().remove(0);
        query("UPDATE albert_login_accounts SET role='user' WHERE id='account'")
            .execute(service.database().pool())
            .await
            .unwrap();
        assert!(write.call("{}".into()).await.is_err());
        assert_eq!(fake.0.lock().unwrap().calls.len(), 2);
        registry.close().await;
        task.abort();
    }
    #[tokio::test]
    async fn refreshing_contracts_or_disabling_servers_rejects_captured_handles_before_a_remote_call(
    ) {
        let (dir, _service, actor) = actor().await;
        let fake = Fake::default();
        let (url, task) = server(fake.clone()).await;
        let registry = McpRegistry::new(dir.path().into(), Arc::new(EnvironmentCredentials));
        let mut config = config(url, &["read"]);
        registry.configure(config.clone()).await.unwrap();
        connected(&registry).await;
        let old = registry.snapshot(Some(&actor)).unwrap().remove(0);
        fake.0.lock().unwrap().version = 2;
        registry.refresh("local").await.unwrap();
        assert!(old.call("{}".into()).await.is_err());
        assert!(fake.0.lock().unwrap().calls.is_empty());
        let fresh = registry.snapshot(Some(&actor)).unwrap().remove(0);
        assert_eq!(fresh.name(), old.name());
        assert_eq!(
            fresh.definition(String::new()).await.parameters["properties"]["version"]["const"],
            2
        );
        config.servers[0].enabled = false;
        registry.configure(config).await.unwrap();
        assert!(fresh.call("{}".into()).await.is_err());
        assert!(registry.snapshot(Some(&actor)).unwrap().is_empty());
        registry.close().await;
        task.abort();
    }
    #[test]
    fn function_names_are_provider_safe_and_slug_collisions_keep_distinct_remote_identities() {
        assert_ne!(
            tool_name("a-b", "namespace/name"),
            tool_name("a_b", "namespace-name")
        );
        let name = tool_name(&"server".repeat(10), &"long tool".repeat(30));
        assert!(name.len() <= 64);
        assert!(name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
        assert_eq!(
            name,
            tool_name(&"server".repeat(10), &"long tool".repeat(30))
        );
    }
    #[tokio::test]
    async fn actual_cogitator_advertises_and_invokes_mcp_as_a_native_function() {
        use crate::cogitator::testing::{install_mcp, sessions::Fixture};
        use octo_core::{
            ChannelId, ChannelMetadata, CogitatorContext, ConnectorCapabilities, ConnectorId,
            ConnectorInfo, Envelope, EventKind,
        };
        let mut fixture = Fixture::with_model("mcp-native").await;
        let fake = Fake::default();
        let (url, server) = server(fake.clone()).await;
        let registry = McpRegistry::new(
            fixture._directory.path().into(),
            Arc::new(EnvironmentCredentials),
        );
        registry.configure(config(url, &["read"])).await.unwrap();
        connected(&registry).await;
        install_mcp(&mut fixture.host, registry.clone());
        let mut connectors = fixture.ctx.connectors().to_vec();
        connectors.push(ConnectorInfo {
            id: ConnectorId::new("web"),
            capabilities: ConnectorCapabilities::bidirectional()
                .with_emit_kinds([EventKind::new("chat.message")]),
        });
        fixture.ctx =
            CogitatorContext::new(fixture.ctx.shutdown.clone(), fixture.ctx.bus(), connectors);
        let mut tx = fixture.service.history().pool().begin().await.unwrap();
        bind(&mut tx, "web", "account", "person").await.unwrap();
        tx.commit().await.unwrap();
        query("INSERT INTO albert_login_accounts(id,username,password_hash,role,canonical_user_id,must_change_password) VALUES ('account','fixture','unused','owner','person',0)").execute(fixture.service.database().pool()).await.unwrap();
        let actor = WebActor {
            account_id: "account".into(),
            user_id: UserId::parse("person").unwrap(),
            tools: ActorToolPolicy::ExistingHost,
        };
        let session = fixture
            .service
            .web_create(&actor, "Native MCP", false)
            .await
            .unwrap();
        let context = fixture
            .service
            .web_context(&actor, &session.id)
            .await
            .unwrap()
            .with_account(fixture.service.accounts().account("account").await.unwrap());
        let message = Envelope::new(
            ConnectorId::new("web"),
            EventKind::new("chat.message"),
            "Use the native read function".to_owned(),
        )
        .with_channel(ChannelId::new(context.reply_route().channel()))
        .with_channel_metadata(
            ChannelMetadata::default()
                .with_tag("chat_type", "private")
                .with_tag("sender_id", "account")
                .with_tag("message_id", "native-mcp")
                .with_tag("role", "owner"),
        )
        .with_context(context);
        let mut replies = fixture.replies().await;
        fixture
            .host
            .clone()
            .panel_handle(Arc::new(message), &fixture.ctx)
            .await;
        timeout(Duration::from_secs(3), async {
            loop {
                if replies.next().await.unwrap().kind.as_str() == "chat.reply" {
                    break;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(fake.0.lock().unwrap().calls, vec!["read"]);
        let requests = fixture.server.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[1]["messages"]
            .to_string()
            .contains("structuredContent"));
        drop(requests);
        registry.close().await;
        server.abort();
    }
}
