//! Validated, revisioned owner configuration. Files remain the interoperable
//! source of settings; application outcomes are durable and idempotent.
use crate::service::panel_models::files::ensure_directory_mode;
use albert_core::mcp::RegistryConfig;
mod applications;
mod candidate;
mod catalog;
mod connector_fields;
mod connectors;
mod documents;
mod drafts;
#[cfg(test)]
mod fixture;
mod redaction;
mod telegram;

use self::redaction::redact;

use std::sync::Arc;

use albert_core::{
    config::Config, models::PoolConfig, runtime::AgentRuntime, selfconfig::SelfConfig,
};
use tokio::sync::Mutex;
use toml::{from_str, Table};

use crate::{
    api::errors::ApiError, api::errors::ApiResult, api::host::ConfigOperation,
    api::host::HostResponse, api::models::ConfigDocument, api::models::ConfigList,
    api::models::ConfigWrite, api::models::EmptyObject, api::models::SecretList,
    api::models::ValidationView, api::principal::OwnerPermit,
    service::panel_models::files::atomic_write, service::panel_models::files::hash,
    service::panel_models::files::read_optional, service::panel_models::files::root,
    service::panel_models::ModelService, settings::WebSettings,
};

pub(crate) struct ControlService {
    pub(crate) runtime: Arc<AgentRuntime>,
    pub(crate) models: ModelService,
    pub(crate) gate: Arc<Mutex<()>>,
}

impl ControlService {
    pub fn new(runtime: Arc<AgentRuntime>, gate: Arc<Mutex<()>>) -> Self {
        Self {
            models: ModelService::new(runtime.path.clone(), runtime.base.clone(), gate.clone()),
            runtime,
            gate,
        }
    }

    pub(crate) fn revision(&self) -> ApiResult<String> {
        let mut content = Vec::new();
        for id in self.documents()?.keys().chain(
            ["soul", "system"]
                .iter()
                .map(|s| *s)
                .map(str::to_owned)
                .collect::<Vec<_>>()
                .iter(),
        ) {
            let doc = self.document(id)?;
            content.extend_from_slice(id.as_bytes());
            content.extend_from_slice(doc.revision.as_bytes());
        }
        let directory = root(&self.runtime.base.deploy_dir)?;
        let secrets =
            read_optional(&directory, &directory.join(".env"), 1024 * 1024)?.unwrap_or_default();
        Ok(hash(&[&content, &secrets]))
    }

    fn validate(&self, input: &ConfigWrite) -> ValidationView {
        let error = if input.content.len() > 1024 * 1024 {
            Some("Configuration exceeds 1 MiB.".into())
        } else if matches!(input.path.as_str(), "soul" | "system") {
            None
        } else if from_str::<Table>(&input.content).is_err() {
            Some("Invalid TOML. Correct syntax before saving.".into())
        } else if input.path == "mcp" {
            from_str::<RegistryConfig>(&input.content)
                .map_err(|_| "Invalid MCP configuration.".to_owned())
                .and_then(|config| {
                    config
                        .validate(&self.runtime.base.deploy_dir)
                        .map_err(|_| "Invalid MCP transport or limits.".to_owned())
                })
                .err()
        } else if input.path == "models" {
            PoolConfig::parse(&input.content).err()
        } else if input.path.starts_with("connector:") {
            self.validate_connector(&input.path, &input.content).err()
        } else if input.path == "albert" {
            from_str::<Table>(&input.content)
                .ok()
                .and_then(|doc| WebSettings::from_document(&doc).err())
                .map(|_| "Invalid web listener or authentication settings.".into())
                .or_else(|| {
                    Config::parse_text(&input.content, &self.runtime.path)
                        .err()
                        .map(|_| {
                            "Invalid agent settings, model pool or memory configuration.".into()
                        })
                })
                .or_else(|| self.validate_storage(&input.content).err())
        } else {
            None
        };
        ValidationView {
            valid: error.is_none(),
            issues: error.into_iter().collect(),
            requires_restart: input.path == "albert"
                || input.path == "connectors"
                || input.path.starts_with("connector:"),
        }
    }

    fn validate_storage(&self, text: &str) -> Result<(), String> {
        let doc: Table = from_str(text).map_err(|_| "Invalid settings.".to_owned())?;
        let history = doc
            .get("history")
            .and_then(|v| v.get("backend"))
            .and_then(|v| v.as_str())
            .unwrap_or("sqlite:state/history.db");
        if Some(history)
            != Some(
                self.runtime
                    .base
                    .history
                    .as_deref()
                    .unwrap_or("sqlite:state/history.db"),
            )
        {
            return Err(
                "Internal history storage cannot be relocated by a live web configuration change."
                    .into(),
            );
        }
        Ok(())
    }

    async fn put(&self, id: &str, input: ConfigWrite) -> ApiResult<ConfigDocument> {
        if input.path != id {
            return Err(ApiError::invalid(
                "Document path must match the resource ID.",
            ));
        }
        let _gate = self.gate.lock().await;
        self.put_locked(id, input)
    }

    fn put_locked(&self, id: &str, input: ConfigWrite) -> ApiResult<ConfigDocument> {
        let old = self.document(id)?;
        if input.revision.as_deref() != Some(old.revision.as_str()) {
            return Err(ApiError::conflict(
                "Configuration changed. Reload before saving.",
            ));
        }
        if old.redacted && input.content.contains("[REDACTED]") {
            return Err(ApiError::invalid("Inline secrets must be moved to named environment references before editing this document."));
        }
        let validation = self.validate(&input);
        if !validation.valid {
            return Err(ApiError::invalid(validation.issues.join(" ")));
        }
        let directory = root(&self.runtime.base.deploy_dir)?;
        if id.starts_with("connector:") {
            self.ensure_topology()?;
        }
        if id == "mcp" {
            ensure_directory_mode(
                &directory,
                self.document_path(id)?
                    .parent()
                    .ok_or_else(ApiError::internal)?,
                0o700,
            )?;
        }
        if id == "models" {
            self.models.replace_document_in(&input.content)?;
        } else {
            atomic_write(
                &directory,
                &self.document_path(id)?,
                input.content.as_bytes(),
            )?;
        }
        self.document(id)
    }

    pub async fn operation(
        &self,
        permit: &OwnerPermit,
        operation: ConfigOperation,
    ) -> ApiResult<HostResponse> {
        match operation {
            ConfigOperation::List => Ok(HostResponse::Configs(ConfigList {
                items: self.documents()?.keys().cloned().collect(),
                revision: self.revision()?,
            })),
            ConfigOperation::Get { id } | ConfigOperation::GetPrompt { id } => {
                Ok(HostResponse::Config(self.document(&id)?))
            }
            ConfigOperation::Put { id, input } | ConfigOperation::PutPrompt { id, input } => {
                Ok(HostResponse::Config(self.put(&id, input).await?))
            }
            ConfigOperation::Validate(input) => Ok(HostResponse::Validation(self.validate(&input))),
            ConfigOperation::Draft { id, input } => {
                Ok(HostResponse::ConfigDraft(self.draft(&id, input).await?))
            }
            ConfigOperation::Apply(input) => {
                Ok(HostResponse::Apply(self.apply(permit, input).await?))
            }
            ConfigOperation::Application { id } => {
                Ok(HostResponse::Apply(self.application(permit, &id).await?))
            }
            ConfigOperation::Secrets => Ok(HostResponse::Secrets(SecretList {
                items: SelfConfig::new(self.runtime.base.deploy_dir.clone()).panel_secret_names(),
            })),
            ConfigOperation::PutSecret { id, input } => {
                let _gate = self.gate.lock().await;
                SelfConfig::new(self.runtime.base.deploy_dir.clone())
                    .panel_set_secret(&id, &input.value)
                    .map_err(|_| ApiError::invalid("Invalid secret name or value."))?;
                Ok(HostResponse::Empty(EmptyObject {}))
            }
            ConfigOperation::DeleteSecret { id } => {
                let _gate = self.gate.lock().await;
                SelfConfig::new(self.runtime.base.deploy_dir.clone())
                    .panel_set_secret(&id, "")
                    .map_err(|_| ApiError::invalid("Invalid secret name."))?;
                Ok(HostResponse::Empty(EmptyObject {}))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs::write, sync::atomic::Ordering};

    use albert_core::config::Config;

    use super::fixture::fixture;
    use crate::{api::models::ApplyRequest, api::models::ConfigWrite};

    #[tokio::test]
    async fn configuration_cas_rejects_stale_writes_and_invalid_web_before_mutation() {
        let (dir, service, _caller) = fixture().await;
        let original = service.document("system").unwrap();
        let saved = service
            .put(
                "system",
                ConfigWrite {
                    path: "system".into(),
                    content: "Instruction one".into(),
                    revision: Some(original.revision.clone()),
                },
            )
            .await
            .unwrap();
        assert_ne!(saved.revision, original.revision);
        assert!(service
            .put(
                "system",
                ConfigWrite {
                    path: "system".into(),
                    content: "stale instruction".into(),
                    revision: Some(original.revision)
                }
            )
            .await
            .is_err());
        assert_eq!(
            service.document("system").unwrap().content,
            "Instruction one"
        );
        let original = service.document("albert").unwrap();
        for text in [
            "[web]\nbind='invalid'",
            "[web]\npassword_workers=0",
            "[history]\nbackend='sqlite:other.db'",
        ] {
            assert!(service
                .put(
                    "albert",
                    ConfigWrite {
                        path: "albert".into(),
                        content: text.into(),
                        revision: Some(original.revision.clone())
                    }
                )
                .await
                .is_err());
        }
        assert!(!dir.path().join("albert.toml").exists());
        assert!(service
            .put(
                "albert",
                ConfigWrite {
                    path: "albert".into(),
                    content: "[web]\nbind='127.0.0.1:9877'".into(),
                    revision: Some(original.revision)
                }
            )
            .await
            .is_ok());
    }
    #[tokio::test]
    async fn application_retries_replay_original_record_and_reject_request_id_rebinding() {
        let (_dir, service, caller) = fixture().await;
        let permit = caller.owner_permit().unwrap();
        let revision = service.document("system").unwrap().revision;
        let input = || ApplyRequest {
            target: "runtime".into(),
            revision: Some(revision.clone()),
            client_request_id: "stable-request".into(),
            document_id: Some("system".into()),
        };
        let first = service.apply(&permit, input()).await.unwrap();
        let second = service.apply(&permit, input()).await.unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(first.state, second.state);
        let mut changed = input();
        changed.target = "process".into();
        assert!(service.apply(&permit, changed).await.is_err());
        let stale = ApplyRequest {
            target: "runtime".into(),
            revision: Some("outdated".into()),
            client_request_id: "new-request".into(),
            document_id: Some("system".into()),
        };
        assert!(service.apply(&permit, stale).await.is_err());
        write(&service.runtime.path, "[web]\nbind='invalid'").unwrap();
        let request = ApplyRequest {
            target: "process".into(),
            revision: Some(service.document("albert").unwrap().revision),
            client_request_id: "invalid-settings".into(),
            document_id: Some("albert".into()),
        };
        let failed = service.apply(&permit, request).await.unwrap();
        assert_eq!(failed.state, "failed");
        assert!(!service.runtime.restart_requested.load(Ordering::Acquire));
        assert!(Config::parse_text("", &service.runtime.path).is_ok());
    }
    #[tokio::test]
    async fn first_connector_creation_initializes_topology_and_preserves_cas_and_id_boundaries() {
        let (_dir, service, _) = fixture().await;
        let topology = service.document("connectors").unwrap();
        service
            .put(
                "connectors",
                ConfigWrite {
                    path: "connectors".into(),
                    content: "[connectors]\nexplicit=[]".into(),
                    revision: Some(topology.revision),
                },
            )
            .await
            .unwrap();
        let original = service.document("connector:search").unwrap();
        assert!(original.content.is_empty());
        for id in ["connector:../escape", "connector:web", "connector:"] {
            assert!(service.document(id).is_err());
        }
        let mismatch = ConfigWrite {
            path: "connector:search".into(),
            content: "[connector]\nid='other'\ntype='search'".into(),
            revision: Some(original.revision.clone()),
        };
        assert!(service.put("connector:search", mismatch).await.is_err());
        let input = || ConfigWrite {
            path: "connector:search".into(),
            content: "[connector]\nid='search'\ntype='search'".into(),
            revision: Some(original.revision.clone()),
        };
        let saved = service.put("connector:search", input()).await.unwrap();
        assert_ne!(original.revision, saved.revision);
        assert!(service
            .documents()
            .unwrap()
            .contains_key("connector:search"));
        assert!(service
            .document("connectors")
            .unwrap()
            .content
            .contains("dir = \"connectors\""));
        assert!(service.put("connector:search", input()).await.is_err());
    }
    #[tokio::test]
    async fn raw_pool_save_sets_pointer_and_invalid_pointer_keeps_repair_document_accessible() {
        let (_dir, service, _) = fixture().await;
        let original = service.document("models").unwrap();
        let content="default='test'\n[[models]]\nid='test'\nmodel='mock'\nprovider='api_key'\nvision=false\ntools=true\n";
        service
            .put(
                "models",
                ConfigWrite {
                    path: "models".into(),
                    content: content.into(),
                    revision: Some(original.revision),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            Config::load_from(&service.runtime.path)
                .unwrap()
                .models
                .unwrap()
                .default,
            "test"
        );
        write(&service.runtime.path, "model_pool=123").unwrap();
        assert_eq!(
            service.document("albert").unwrap().content,
            "model_pool=123"
        );
        assert!(service.documents().unwrap().contains_key("albert"));
        let revision = service.document("albert").unwrap().revision;
        service
            .put(
                "albert",
                ConfigWrite {
                    path: "albert".into(),
                    content: "".into(),
                    revision: Some(revision),
                },
            )
            .await
            .unwrap();
    }
}
