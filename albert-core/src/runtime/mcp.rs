use std::{fs::read_to_string, path::PathBuf};

use octo_mcp::Credentials;
use toml::from_str;
use tracing::warn;

use super::AgentRuntime;
use crate::{config::Config, mcp::RegistryConfig};

pub(super) struct ConfigCredentials(pub PathBuf);
impl Credentials for ConfigCredentials {
    fn resolve(&self, reference: &str) -> Option<String> {
        Config::load_control_from(&self.0)
            .ok()
            .and_then(|config| config.secret(reference))
    }
}
impl AgentRuntime {
    pub(super) async fn reload_mcp(&self) {
        let path = Config::load_control_from(&self.path)
            .map(|config| config.mcp_registry)
            .unwrap_or_else(|_| self.base.mcp_registry.clone());
        let config = if path.exists() {
            read_to_string(&path)
                .ok()
                .filter(|text| text.len() <= 1024 * 1024)
                .and_then(|text| from_str::<RegistryConfig>(&text).ok())
        } else {
            Some(RegistryConfig::default())
        };
        if let Some(config) = config {
            if self.mcp.configure(config).await.is_err() {
                warn!("MCP registry configuration could not be applied");
            }
        } else {
            warn!("MCP registry configuration could not be loaded");
        }
    }
}
