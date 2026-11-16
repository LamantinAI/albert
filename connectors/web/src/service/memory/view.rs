use albert_core::{
    db::repo::memory_setup, memory::config::McpTransport, memory::config::MemoryConfig,
    selfconfig::SelfConfig,
};

use super::MemoryService;
use crate::api::{
    errors::ApiError, errors::ApiResult, memory::MemoryPhase, memory::MemorySettings,
    memory::MemoryView,
};

impl MemoryService {
    pub(super) async fn view(&self) -> ApiResult<MemoryView> {
        let config = self.configuration()?;
        let setup = memory_setup::read(self.runtime.sessions.database().pool())
            .await
            .map_err(|_| ApiError::internal())?;
        let status = self.runtime.status();
        let phase = if !setup.selected() {
            MemoryPhase::SelectionPending
        } else if self.runtime.current().is_some() {
            if self.runtime.memory_matches(&config) {
                MemoryPhase::Active
            } else {
                MemoryPhase::RestartRequired
            }
        } else if status.phase == "memory_initializing" {
            MemoryPhase::Initializing
        } else if status.phase == "memory_error" {
            MemoryPhase::InitializationFailed
        } else {
            MemoryPhase::Saved
        };
        let token_configured = match &config.memory {
            MemoryConfig::Mcp {
                transport:
                    McpTransport::Http {
                        token_env: Some(name),
                        ..
                    },
                ..
            } => config.secret(name).is_some_and(|value| !value.is_empty()),
            _ => false,
        };
        Ok(MemoryView {
            config: MemorySettings::from(&config.memory),
            revision: self.control().revision()?,
            selection_required: !setup.selected(),
            model_configured: status.model_configured,
            token_configured,
            credential_names: SelfConfig::new(config.deploy_dir).panel_secret_names(),
            error: matches!(phase, MemoryPhase::InitializationFailed).then(|| "Memory could not be initialized. Review the connection and credential references, then retry. No fallback backend was opened.".into()),
            phase,
            document: None,
        })
    }
}
