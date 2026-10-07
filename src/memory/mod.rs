//! Memory has one selected backend. Both expose kaeru verbs to cognition.

pub mod config;
mod delegation;
mod mcp;
mod migration;

mod tool;

use std::{collections::HashMap, env::var, sync::Arc};

use kaeru_core::{KaeruConfig, Store};
use kaeru_rig::{CloudClient, CloudRegistry, KaeruMemory};
use rig::{
    agent::{AgentBuilder, NoToolConfig, WithBuilderTools},
    completion::CompletionModel,
    tool::ToolDyn,
};
use thiserror::Error;
use tokio::task::{spawn_blocking, JoinError};
use tracing::{info, warn};

use self::{config::MemoryConfig, delegation::embedded_tools, mcp::McpMemory, migration::embedded};
use crate::config::Config;

pub const INITIATIVE: &str = "albert";

#[derive(Debug, Error)]
pub enum MemoryError {
    #[error("memory config: {0}")]
    Config(String),
    #[error("embedded memory: {0}")]
    Embedded(String),
    #[error("memory MCP connection: {0}")]
    Connection(String),
    #[error("memory MCP protocol: {0}")]
    Protocol(String),
    #[error("memory MCP operation timed out")]
    Timeout,
    #[error("memory MCP tool failed: {0}")]
    Tool(String),
    #[error("memory task: {0}")]
    Task(#[from] JoinError),
}

pub enum Memory {
    Embedded { memory: KaeruMemory, clouds: bool },
    Mcp(Arc<McpMemory>),
}

impl Memory {
    pub async fn open(config: &Config) -> Result<Self, MemoryError> {
        if let MemoryConfig::Mcp {
            timeout_secs,
            transport,
        } = &config.memory
        {
            let memory = McpMemory::open(transport.clone(), *timeout_secs).await?;
            info!(
                initiative = INITIATIVE,
                "memory: MCP connected and migrated"
            );
            return Ok(Self::Mcp(memory));
        }
        let store = spawn_blocking(|| {
            let config =
                KaeruConfig::from_env().map_err(|e| MemoryError::Embedded(e.to_string()))?;
            let store = Store::open_with_config(config)
                .map_err(|e| MemoryError::Embedded(e.to_string()))?;
            Ok::<_, MemoryError>(Arc::new(store))
        })
        .await??;
        let clouds = !config.clouds.is_empty();
        let memory = if clouds {
            let clients: HashMap<String, CloudClient> = config
                .clouds
                .iter()
                .map(|(name, endpoint)| {
                    let token = var(&endpoint.token_env).unwrap_or_default();
                    if token.is_empty() {
                        warn!(cloud = %name, env = %endpoint.token_env, "cloud token env is unset");
                    }
                    (
                        name.clone(),
                        CloudClient::new(name.clone(), endpoint.url.clone(), token),
                    )
                })
                .collect();
            KaeruMemory::with_clouds(
                store,
                INITIATIVE,
                CloudRegistry::new(clients, config.clouds_default.clone()),
            )
        } else {
            KaeruMemory::with_initiative(store, INITIATIVE)
        };
        embedded(&memory).await?;
        info!(initiative = INITIATIVE, clouds, "memory: embedded kaeru");
        Ok(Self::Embedded { memory, clouds })
    }

    /// Individually grantable native verbs. No bulk installation in children.
    pub fn delegation_tools(&self) -> Vec<Box<dyn ToolDyn>> {
        match self {
            Self::Mcp(memory) => memory.tools(),
            Self::Embedded { memory, clouds } => embedded_tools(memory, *clouds),
        }
    }

    pub fn install<M: CompletionModel + 'static>(
        &self,
        base: AgentBuilder<M, (), NoToolConfig>,
    ) -> AgentBuilder<M, (), WithBuilderTools> {
        match self {
            Self::Embedded {
                memory,
                clouds: true,
            } => memory.install_with_cloud(base),
            Self::Embedded {
                memory,
                clouds: false,
            } => memory.install(base),
            Self::Mcp(memory) => base.tools(memory.tools()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{path::Path, sync::Arc};

    use kaeru_core::{cite, list_initiatives, read_node_full, recall_id_by_name, Store};
    use kaeru_rig::KaeruMemory;
    use rmcp::model::{CallToolResult, Content};
    use toml::from_str;

    use super::{
        config::{McpTransport, MemoryConfig},
        migration::{embedded, has_initiative, NAME},
        INITIATIVE,
    };

    #[test]
    fn config_defaults_and_transport_validation() {
        let mut http: MemoryConfig =
            from_str("backend = 'mcp'\ntransport = 'http'\nurl = 'http://localhost:9876/mcp'")
                .unwrap();
        http.validate(Path::new("/deploy"), false).unwrap();
        assert!(http.validate(Path::new("/deploy"), true).is_err());
        let mut stdio: MemoryConfig = from_str(
            "backend = 'mcp'\ntransport = 'stdio'\ncommand = './bin/kaeru-mcp'\nargs = ['--stdio']",
        )
        .unwrap();
        stdio.validate(Path::new("/deploy"), false).unwrap();
        assert!(
            matches!(stdio, MemoryConfig::Mcp { transport: McpTransport::Stdio { command, .. }, .. } if Path::new(&command) == Path::new("/deploy/bin/kaeru-mcp"))
        );
        assert!(matches!(MemoryConfig::default(), MemoryConfig::Embedded));
        for text in [
            "backend = 'other'",
            "backend = 'mcp'\ntransport = 'http'",
            "backend = 'embedded'\nurl = 'http://ignored'",
        ] {
            assert!(from_str::<MemoryConfig>(text).is_err(), "{text}");
        }
        for (url, seconds) in [
            ("file:///tmp/memory", 30),
            ("http://u:secret@host/mcp", 30),
            ("http://localhost/mcp", 0),
        ] {
            let mut cfg = MemoryConfig::Mcp {
                timeout_secs: seconds,
                transport: McpTransport::Http {
                    url: url.into(),
                    token_env: None,
                },
            };
            assert!(cfg.validate(Path::new("."), false).is_err());
        }
    }

    #[tokio::test]
    async fn native_migration_is_idempotent_and_does_not_change_existing_memory() {
        let store = Arc::new(Store::open_in_memory().unwrap());
        let memory = KaeruMemory::with_initiative(store.clone(), INITIATIVE);
        embedded(&memory).await.unwrap();
        let id = store.scoped(Some(INITIATIVE), |s| {
            recall_id_by_name(s, NAME).unwrap().unwrap()
        });
        let before = read_node_full(&store, &id).unwrap().unwrap();
        embedded(&memory).await.unwrap();
        let after = read_node_full(&store, &id).unwrap().unwrap();
        assert_eq!(before.body, after.body);
        assert_eq!(list_initiatives(&store).unwrap(), [INITIATIVE]);

        let existing = Arc::new(Store::open_in_memory().unwrap());
        existing.scoped(Some(INITIATIVE), |s| {
            cite(s, "user-fact", None, "keep me").unwrap()
        });
        embedded(&KaeruMemory::with_initiative(existing.clone(), INITIATIVE))
            .await
            .unwrap();
        assert!(existing
            .scoped(Some(INITIATIVE), |s| recall_id_by_name(s, NAME).unwrap())
            .is_none());
    }

    #[test]
    fn migration_fails_closed_on_unexpected_or_incomplete_responses() {
        let result = |text| CallToolResult::success(vec![Content::text(text)]);
        assert!(
            has_initiative(&result("initiatives (2):\n  - albert-archive\n  - kaeru"))
                .is_ok_and(|v| !v)
        );
        assert!(has_initiative(&result("initiatives (2):\n  - albert\n  - kaeru")).unwrap());
        for text in [
            "database offline",
            "initiatives (2):\n  - kaeru",
            "initiatives (1):\n  albert",
        ] {
            assert!(has_initiative(&result(text)).is_err());
        }
        assert!(has_initiative(&CallToolResult::error(vec![Content::text("offline")])).is_err());
    }
    #[tokio::test]
    async fn delegation_candidates_match_installed_memory_surface() {
        for clouds in [false, true] {
            let memory =
                KaeruMemory::with_initiative(Arc::new(Store::open_in_memory().unwrap()), "albert");
            let mut expected: Vec<_> = memory
                .local_tool_definitions()
                .await
                .into_iter()
                .map(|t| t.name)
                .collect();
            if clouds {
                expected.extend(
                    memory
                        .cloud_tool_definitions()
                        .await
                        .into_iter()
                        .map(|t| t.name),
                );
            }
            let backend = super::Memory::Embedded { memory, clouds };
            let mut names: Vec<_> = backend
                .delegation_tools()
                .iter()
                .map(|t| t.name())
                .collect();
            expected.sort();
            names.sort();
            assert_eq!(names, expected);
        }
    }
}
