use albert_core::{
    config::Config, memory::config::McpTransport, memory::config::MemoryConfig,
    selfconfig::SelfConfig,
};
use axum::http::StatusCode;
use toml::{from_str, to_string, Table};
use toml_edit::{DocumentMut, Item};

use crate::{
    api::errors::ApiError, api::errors::ApiResult, api::memory::MemoryBackend,
    api::memory::MemorySettings, api::memory::MemoryTransport, api::memory::MemoryWrite,
    service::panel_models::files::atomic_write, service::panel_models::files::read_optional,
    service::panel_models::files::root,
};

use super::MemoryService;

impl From<&MemoryConfig> for MemorySettings {
    fn from(config: &MemoryConfig) -> Self {
        let mut result = Self {
            backend: MemoryBackend::Embedded,
            transport: None,
            timeout_secs: None,
            url: None,
            token_env: None,
            command: None,
            args: None,
            env: None,
        };
        if let MemoryConfig::Mcp {
            timeout_secs,
            transport,
        } = config
        {
            result.backend = MemoryBackend::Mcp;
            result.timeout_secs = Some(*timeout_secs);
            match transport {
                McpTransport::Http { url, token_env } => {
                    result.transport = Some(MemoryTransport::Http);
                    result.url = Some(url.clone());
                    result.token_env = token_env.clone();
                }
                McpTransport::Stdio { command, args, env } => {
                    result.transport = Some(MemoryTransport::Stdio);
                    result.command = Some(command.clone());
                    result.args = Some(args.clone());
                    result.env = Some(env.iter().map(|(k, v)| (k.clone(), v.clone())).collect());
                }
            }
        }
        result
    }
}
impl MemoryService {
    pub(super) fn configuration(&self) -> ApiResult<Config> {
        let directory = root(&self.runtime.base.deploy_dir)?;
        let bytes = read_optional(&directory, &self.runtime.path, 1024 * 1024)?.unwrap_or_default();
        let text = String::from_utf8(bytes)
            .map_err(|_| ApiError::invalid("Agent configuration must be UTF-8."))?;
        Config::parse_text(&text, &self.runtime.path).map_err(|_| {
            ApiError::invalid("Repair the agent configuration in Advanced before editing memory.")
        })
    }
    pub(super) fn save(&self, mut input: MemoryWrite) -> ApiResult<()> {
        if let Some(token) = &input.token_value {
            if !matches!(input.config.transport, Some(MemoryTransport::Http))
                || !matches!(input.config.backend, MemoryBackend::Mcp)
                || token.trim().is_empty()
                || token.contains(['\n', '\r', '\0'])
            {
                return Err(ApiError::invalid(
                    "A bearer token requires MCP over HTTP and a nonempty single-line value.",
                ));
            }
            input
                .config
                .token_env
                .get_or_insert_with(|| "ALBERT_MEMORY_MCP_TOKEN".into());
        }
        validate_references(&input.config)?;
        let directory = root(&self.runtime.base.deploy_dir)?;
        let bytes = read_optional(&directory, &self.runtime.path, 1024 * 1024)?.unwrap_or_default();
        let text = String::from_utf8(bytes)
            .map_err(|_| ApiError::invalid("Agent configuration must be UTF-8."))?;
        let source = if let Some(candidate) = &input.document {
            self.control()
                .check_agent_candidate(&candidate.content, &candidate.revision)?;
            let current: Table = from_str(&text).map_err(|_| {
                ApiError::invalid("Repair the Agent document before saving memory.")
            })?;
            let proposed: Table = from_str(&candidate.content)
                .map_err(|_| ApiError::invalid("Correct the Agent document before saving."))?;
            if current.get("memory") != proposed.get("memory") {
                return Err(ApiError::new(StatusCode::CONFLICT, "memory_document_overlap", "Memory was edited in both the document and the Memory section. Keep one version before saving."));
            }
            &candidate.content
        } else {
            &text
        };
        let mut document = source.parse::<DocumentMut>().map_err(|_| {
            ApiError::invalid("Repair the agent configuration in Advanced before changing memory.")
        })?;
        let table = to_string(&input.config)
            .map_err(|_| ApiError::internal())?
            .parse::<DocumentMut>()
            .map_err(|_| ApiError::internal())?;
        document["memory"] = Item::Table(table.as_table().clone());
        let text = document.to_string();
        self.control().validate_agent_candidate(&text, None)?;
        if let Some(token) = input.token_value {
            SelfConfig::new(self.runtime.base.deploy_dir.clone())
                .panel_set_secret(
                    input
                        .config
                        .token_env
                        .as_deref()
                        .ok_or_else(ApiError::internal)?,
                    &token,
                )
                .map_err(|_| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "memory_credentials_uncertain", "The credential update could not be confirmed. Review the saved settings before explicitly retrying."))?;
        }
        atomic_write(&directory, &self.runtime.path, text.as_bytes()).map_err(|_| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "memory_save_uncertain", "The settings write could not be confirmed. A submitted credential may already have changed. Review the current settings before retrying."))
    }
}
fn validate_references(input: &MemorySettings) -> ApiResult<()> {
    // The core parser remains the authority for backend/transport combinations.
    from_str::<MemoryConfig>(&to_string(input).map_err(|_| ApiError::internal())?)
        .map_err(|_| ApiError::invalid("Invalid memory backend or transport settings."))?;
    let name = |name: &str| {
        !name.is_empty()
            && name
                .chars()
                .enumerate()
                .all(|(i, c)| c == '_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit()))
    };
    if input.token_env.as_ref().is_some_and(|s| !name(s))
        || input
            .env
            .as_ref()
            .is_some_and(|env| env.iter().any(|(key, value)| !name(key) || !name(value)))
    {
        return Err(ApiError::invalid(
            "Credential references must be environment variable names, never secret values.",
        ));
    }
    Ok(())
}
