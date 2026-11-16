use std::{path::PathBuf, str::from_utf8};

use albert_core::{config::Config, mcp::RegistryConfig, mcp::ServerSpec};
use axum::http::StatusCode;
use serde_json::{from_value, to_value};
use toml::{from_str, to_string_pretty};

use super::McpService;
use crate::{
    api::errors::ApiError, api::errors::ApiResult, api::mcp::McpOperation,
    api::models::McpServerConfig, service::panel_models::files::atomic_write,
    service::panel_models::files::checked, service::panel_models::files::ensure_directory_mode,
    service::panel_models::files::hash, service::panel_models::files::read_optional,
    service::panel_models::files::root,
};

pub(super) struct Loaded {
    pub root: PathBuf,
    pub path: PathBuf,
    pub config: RegistryConfig,
    pub revision: String,
}
impl McpService {
    pub(super) fn load(&self) -> ApiResult<Loaded> {
        let base = Config::load_control_from(&self.runtime.path).map_err(|_| {
            ApiError::invalid("Agent configuration must be repaired before editing MCP settings.")
        })?;
        let root = root(&base.deploy_dir)?;
        let path = checked(&root, &base.mcp_registry)?;
        let bytes = read_optional(&root, &path, 1024 * 1024)?;
        let config = if let Some(bytes) = &bytes {
            from_str(
                from_utf8(bytes)
                    .map_err(|_| ApiError::invalid("MCP configuration must be UTF-8."))?,
            )
            .map_err(|_| {
                ApiError::invalid("MCP configuration is invalid. Open Advanced to repair it.")
            })?
        } else {
            RegistryConfig::default()
        };
        let base_bytes = read_optional(&root, &self.runtime.path, 1024 * 1024)?.unwrap_or_default();
        let revision = hash(&[
            &base_bytes,
            path.strip_prefix(&root)
                .map_err(|_| ApiError::internal())?
                .as_os_str()
                .as_encoded_bytes(),
            bytes.as_deref().unwrap_or_default(),
        ]);
        Ok(Loaded {
            root,
            path,
            config,
            revision,
        })
    }
    pub(super) fn mutate(&self, operation: McpOperation) -> ApiResult<RegistryConfig> {
        let mut loaded = self.load()?;
        match operation {
            McpOperation::Create(input) => {
                revision(&input.revision, &loaded.revision)?;
                if loaded
                    .config
                    .servers
                    .iter()
                    .any(|server| server.id == input.config.id)
                {
                    return Err(ApiError::conflict(
                        "An MCP server with this ID already exists.",
                    ));
                }
                loaded.config.servers.push(spec(input.config)?);
            }
            McpOperation::Update { id, input } => {
                revision(&input.revision, &loaded.revision)?;
                if id != input.config.id {
                    return Err(ApiError::invalid("The server ID must match its resource."));
                }
                let server = loaded
                    .config
                    .servers
                    .iter_mut()
                    .find(|server| server.id == id)
                    .ok_or_else(ApiError::not_found)?;
                *server = spec(input.config)?;
            }
            McpOperation::Delete {
                id,
                revision: expected,
            } => {
                revision(expected.as_deref().unwrap_or(""), &loaded.revision)?;
                let index = loaded
                    .config
                    .servers
                    .iter()
                    .position(|server| server.id == id)
                    .ok_or_else(ApiError::not_found)?;
                loaded.config.servers.remove(index);
            }
            _ => return Err(ApiError::invalid("Invalid MCP configuration mutation.")),
        }
        loaded
            .config
            .validate(&loaded.root)
            .map_err(|_| ApiError::invalid("Invalid MCP transport, limits or access settings."))?;
        let content = to_string_pretty(&loaded.config).map_err(|_| ApiError::internal())?;
        if content.len() > 1024 * 1024 {
            return Err(ApiError::invalid(
                "The MCP configuration exceeds its size limit.",
            ));
        }
        ensure_directory_mode(
            &loaded.root,
            loaded.path.parent().ok_or_else(ApiError::internal)?,
            0o700,
        )?;
        atomic_write(&loaded.root, &loaded.path, content.as_bytes())?;
        Ok(loaded.config)
    }
}
fn spec(input: McpServerConfig) -> ApiResult<ServerSpec> {
    from_value(to_value(input).map_err(|_| ApiError::internal())?)
        .map_err(|_| ApiError::invalid("Invalid MCP connection settings."))
}
fn revision(expected: &str, current: &str) -> ApiResult<()> {
    if expected == current {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::PRECONDITION_FAILED,
            "revision_changed",
            "MCP settings changed. Reload before saving.",
        ))
    }
}
