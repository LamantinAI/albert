//! Memory has one selected backend. Both expose kaeru verbs to cognition.

pub mod config;
mod mcp;
mod migration;
#[cfg(test)]
mod tests;
mod tool;

use std::{collections::HashMap, env::var, sync::Arc};

use kaeru_core::{KaeruConfig, Store};
use kaeru_rig::{CloudClient, CloudRegistry, KaeruMemory};
use rig::{
    agent::{AgentBuilder, NoToolConfig, WithBuilderTools},
    completion::CompletionModel,
};
use thiserror::Error;
use tokio::task::{spawn_blocking, JoinError};
use tracing::{info, warn};

use self::{config::MemoryConfig, mcp::McpMemory, migration::embedded};
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
