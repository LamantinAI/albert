use std::{collections::HashSet, path::Path};

use octo_mcp::{ClientLimits, ServerConfig};
use serde::{Deserialize, Serialize};

use super::{McpRegistryError, Result};

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ServerSpec {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "enabled")]
    pub enabled: bool,
    pub connection: ServerConfig,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
}
#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct RegistryConfig {
    pub limits: ClientLimits,
    pub max_servers: usize,
    pub connect_concurrency: usize,
    pub servers: Vec<ServerSpec>,
}
impl Default for RegistryConfig {
    fn default() -> Self {
        Self {
            limits: ClientLimits::default(),
            max_servers: 32,
            connect_concurrency: 4,
            servers: vec![],
        }
    }
}
impl RegistryConfig {
    pub fn validate(&self, base: &Path) -> Result<()> {
        self.limits.validate()?;
        if !(1..=128).contains(&self.max_servers)
            || !(1..=32).contains(&self.connect_concurrency)
            || self.servers.len() > self.max_servers
        {
            return Err(McpRegistryError::Configuration);
        }
        let mut ids = HashSet::new();
        for server in &self.servers {
            if !valid_id(&server.id)
                || !ids.insert(&server.id)
                || server.name.trim().is_empty()
                || server.name.len() > 256
                || server.description.len() > 4096
                || server.allowed_tools.len() > self.limits.max_tools
            {
                return Err(McpRegistryError::Configuration);
            }
            let mut tools = HashSet::new();
            if server
                .allowed_tools
                .iter()
                .any(|name| name.is_empty() || name.len() > 1024 || !tools.insert(name))
            {
                return Err(McpRegistryError::Configuration);
            }
            server.connection.validate(base)?;
        }
        Ok(())
    }
}
pub fn valid_id(id: &str) -> bool {
    id != "new"
        && !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}
fn enabled() -> bool {
    true
}
