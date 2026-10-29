//! Validated, revisioned owner configuration. Files remain the interoperable
//! source of settings; application outcomes are durable and idempotent.
mod applications;
mod connectors;
#[cfg(test)]
mod fixture;

use std::{collections::BTreeMap, fs::read_to_string, path::PathBuf, sync::Arc};

use tokio::sync::Mutex;
use toml::{from_str, to_string_pretty, Table, Value as TomlValue};

use crate::{
    api::{
        errors::{ApiError, ApiResult},
        host::{ConfigOperation, HostResponse},
        models::{
            ConfigDocument, ConfigList, ConfigWrite, EmptyObject, SecretList, ValidationView,
        },
        principal::OwnerPermit,
    },
    config::{control::WebSettings, Config},
    manifests::connector_files,
    models::PoolConfig,
    selfconfig::SelfConfig,
    service::panel_models::{
        files::{atomic_write, hash, read_optional, root},
        ModelService,
    },
    web::runtime::AgentRuntime,
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

    fn documents(&self) -> ApiResult<BTreeMap<String, PathBuf>> {
        let base = &self.runtime.base;
        let mut docs = BTreeMap::from([
            ("albert".into(), self.runtime.path.clone()),
            ("connectors".into(), base.connectors_manifest.clone()),
        ]);
        if let Ok(path) = self.model_path() {
            docs.insert("models".into(), path);
        }
        for path in connector_files(&base.connectors_manifest) {
            let Ok(text) = read_to_string(&path) else {
                continue;
            };
            let Ok(value) = from_str::<Table>(&text) else {
                continue;
            };
            if let Some(id) = value
                .get("connector")
                .and_then(|v| v.get("id"))
                .and_then(|v| v.as_str())
            {
                if id.len() <= 128 {
                    docs.insert(format!("connector:{id}"), path);
                }
            }
        }
        Ok(docs)
    }

    fn model_path(&self) -> ApiResult<PathBuf> {
        self.models.path()
    }

    fn document_path(&self, id: &str) -> ApiResult<PathBuf> {
        if id == "albert" {
            return Ok(self.runtime.path.clone());
        }
        if id == "connectors" {
            return Ok(self.runtime.base.connectors_manifest.clone());
        }
        if id == "models" {
            return self.model_path();
        }
        if id == "soul" {
            return Ok(self.runtime.base.soul_path.clone());
        }
        if id == "system" {
            return Ok(self.runtime.base.system_path.clone());
        }
        if let Some(path) = self.documents()?.remove(id) {
            return Ok(path);
        }
        self.new_connector_path(id)
    }

    pub(crate) fn document(&self, id: &str) -> ApiResult<ConfigDocument> {
        let directory = root(&self.runtime.base.deploy_dir)?;
        let path = self.document_path(id)?;
        let bytes = read_optional(&directory, &path, 1024 * 1024)?.unwrap_or_default();
        let text = String::from_utf8(bytes.clone())
            .map_err(|_| ApiError::invalid("Configuration must be UTF-8."))?;
        let (content, redacted) = redact(text);
        Ok(ConfigDocument {
            path: id.into(),
            content,
            revision: hash(&[&bytes]),
            redacted,
        })
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

fn redact(text: String) -> (String, bool) {
    let Ok(mut doc) = from_str::<Table>(&text) else {
        return (text, false);
    };
    fn visit(value: &mut TomlValue) -> bool {
        let mut changed = false;
        match value {
            TomlValue::Table(table) => {
                for (key, value) in table {
                    let lower = key.to_ascii_lowercase();
                    if matches!(
                        lower.as_str(),
                        "api_key"
                            | "password"
                            | "access_token"
                            | "refresh_token"
                            | "client_secret"
                            | "token"
                            | "authorization"
                            | "cookie"
                            | "bearer"
                    ) && value.is_str()
                    {
                        *value = TomlValue::String("[REDACTED]".into());
                        changed = true;
                    } else {
                        changed |= visit(value);
                    }
                }
            }
            TomlValue::Array(values) => {
                for value in values {
                    changed |= visit(value);
                }
            }
            _ => {}
        }
        changed
    }
    let mut value = TomlValue::Table(doc.clone());
    let changed = visit(&mut value);
    if changed {
        doc = value.try_into().unwrap_or_default();
        (to_string_pretty(&doc).unwrap_or_default(), true)
    } else {
        (text, false)
    }
}

#[cfg(test)]
mod tests {
    use std::{fs::write, sync::atomic::Ordering};

    use super::fixture::fixture;
    use crate::{
        api::models::{ApplyRequest, ConfigWrite},
        config::Config,
    };

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
