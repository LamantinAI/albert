//! Owner control of persisted model settings. Runtime activation is assembly-owned.
pub(crate) mod files;
mod mutations;
mod views;

use std::{path::PathBuf, sync::Arc};

use tokio::sync::Mutex;

use self::files::serialized;

use crate::{
    api::{errors::ApiResult, host::ModelOperation, models::ModelPoolView, principal::OwnerPermit},
    config::Config,
};

#[derive(Clone)]
pub(crate) struct ModelService {
    config_path: PathBuf,
    base: Config,
    write_gate: Arc<Mutex<()>>,
}

impl ModelService {
    pub fn new(config_path: PathBuf, base: Config, write_gate: Arc<Mutex<()>>) -> Self {
        Self {
            config_path,
            base,
            write_gate,
        }
    }

    pub fn path(&self) -> ApiResult<PathBuf> {
        Ok(self.source()?.path)
    }

    pub async fn list(&self) -> ApiResult<ModelPoolView> {
        let service = self.clone();
        serialized(Arc::clone(&self.write_gate), move || {
            service.view(&service.load()?)
        })
        .await
    }

    pub async fn operation(
        &self,
        _permit: &OwnerPermit,
        operation: ModelOperation,
    ) -> ApiResult<ModelPoolView> {
        let service = self.clone();
        serialized(Arc::clone(&self.write_gate), move || {
            service.mutate(operation)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs::{create_dir_all, read, write},
        os::unix::fs::{symlink, PermissionsExt},
        sync::{mpsc::sync_channel, Arc},
        time::Duration,
    };

    use axum::http::StatusCode;
    use rand::random;
    use serde_json::to_string;
    use tempfile::{tempdir, TempDir};
    use tokio::{
        spawn,
        sync::{oneshot::channel, Mutex},
        time::timeout,
    };
    use toml::to_string_pretty;

    use super::{files::serialized, ModelService};
    use crate::{
        api::{
            host::ModelOperation,
            models::{
                AccountRole, AccountView, ModelConfig, ModelOrder, ModelPreference, ModelWrite,
            },
            principal::{OwnerPermit, Principal},
        },
        config::Config,
        models::{PoolConfig, RoleProfile},
    };

    pub(crate) fn permit() -> OwnerPermit {
        Principal::authenticated(
            AccountView {
                id: "owner".into(),
                username: "owner".into(),
                role: AccountRole::Owner,
                canonical_user_id: Some("owner".into()),
                must_change_password: false,
                disabled: false,
                revision: 1,
            },
            "test-credential".into(),
            "test-csrf".into(),
        )
        .owner_permit()
        .unwrap()
    }

    fn fixture() -> (TempDir, ModelService) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("albert.toml");
        write(
            &path,
            format!(
                "# Preserve this deployment note\nopenai_key_env = \"ALBERT_TEST_UNUSED_{}\"\n",
                random::<u64>()
            ),
        )
        .unwrap();
        let config = Config::load_from(&path).unwrap();
        let service = ModelService::new(path, config, Arc::new(Mutex::new(())));
        (dir, service)
    }

    fn model(id: &str, revision: Option<String>, secret: Option<&str>) -> ModelWrite {
        ModelWrite {
            config: ModelConfig {
                id: id.into(),
                model: format!("provider-{id}"),
                provider: "api_key".into(),
                base_url: Some("http://127.0.0.1:18081/v1".into()),
                api_key_env: None,
                context_window: Some(32_000),
                vision: true,
                tools: true,
                reasoning: None,
                request_timeout_ms: Some(30_000),
            },
            api_key: secret.map(str::to_owned),
            revision,
        }
    }

    #[tokio::test]
    async fn disconnect_does_not_release_a_running_file_transaction() {
        let gate = Arc::new(Mutex::new(()));
        let (started, ready) = channel();
        let (release, wait) = sync_channel(1);
        let task = spawn(serialized(Arc::clone(&gate), move || {
            let _ = started.send(());
            wait.recv_timeout(Duration::from_secs(2)).unwrap();
            Ok(())
        }));
        timeout(Duration::from_secs(2), ready)
            .await
            .unwrap()
            .unwrap();
        task.abort();
        let _ = task.await;
        assert!(gate.try_lock().is_err());
        release.send(()).unwrap();
        let guard = timeout(Duration::from_secs(2), gate.lock()).await.unwrap();
        drop(guard);
    }

    #[tokio::test]
    async fn blank_bootstrap_has_no_fake_model_and_saved_keys_are_write_only() {
        let (dir, service) = fixture();
        assert!(service.list().await.unwrap().models.is_empty());
        let first = service
            .operation(
                &permit(),
                ModelOperation::Create(model("first", None, Some("private-provider-key"))),
            )
            .await
            .unwrap();
        assert!(first.models[0].credential_configured);
        assert!(!to_string(&first).unwrap().contains("private-provider-key"));
        let reference = first.models[0].config.api_key_env.as_ref().unwrap();
        assert!(reference.starts_with("ALBERT_MODEL_FIRST_"));
        let config = Config::load_from(&dir.path().join("albert.toml")).unwrap();
        assert_eq!(
            config.secret(reference).as_deref(),
            Some("private-provider-key")
        );
        assert!(
            String::from_utf8(read(dir.path().join("albert.toml")).unwrap())
                .unwrap()
                .contains("# Preserve this deployment note")
        );
        assert_eq!(
            dir.path()
                .join(".env")
                .metadata()
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let second = service
            .operation(
                &permit(),
                ModelOperation::Create(model("second", Some(first.revision), Some("other-key"))),
            )
            .await
            .unwrap();
        assert_ne!(
            second.models[0].config.api_key_env,
            second.models[1].config.api_key_env
        );
    }

    #[tokio::test]
    async fn priority_and_preference_preserve_role_profiles_and_limits() {
        let (_dir, service) = fixture();
        service
            .operation(&permit(), ModelOperation::Create(model("a", None, None)))
            .await
            .unwrap();
        service
            .operation(&permit(), ModelOperation::Create(model("b", None, None)))
            .await
            .unwrap();
        let path = service.path().unwrap();
        let mut original = PoolConfig::read(&path).unwrap();
        original.max_attempts = 7;
        original.retries_per_model = 3;
        original.roles = vec![RoleProfile {
            id: "review".into(),
            description: "Read before deciding".into(),
            models: vec!["a".into(), "b".into()],
            reasoning_effort: None,
        }];
        write(&path, to_string_pretty(&original).unwrap()).unwrap();
        let before = service.list().await.unwrap();
        let after = service
            .operation(
                &permit(),
                ModelOperation::Order(ModelOrder {
                    ids: vec!["b".into(), "a".into()],
                    revision: before.revision,
                }),
            )
            .await
            .unwrap();
        assert_eq!(after.models[0].config.id, "b");
        assert_eq!(after.preferred.as_deref(), Some("a"));
        service
            .operation(
                &permit(),
                ModelOperation::Preference(ModelPreference {
                    model_id: "b".into(),
                }),
            )
            .await
            .unwrap();
        let persisted = PoolConfig::read(&path).unwrap();
        assert_eq!(persisted.default, "b");
        assert_eq!(persisted.max_attempts, 7);
        assert_eq!(persisted.retries_per_model, 3);
        assert_eq!(persisted.roles[0].models, ["a", "b"]);
        assert_eq!(
            service
                .operation(
                    &permit(),
                    ModelOperation::Delete {
                        id: "a".into(),
                        revision: Some(service.list().await.unwrap().revision),
                    }
                )
                .await
                .err()
                .unwrap()
                .status,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(PoolConfig::read(&path).unwrap().models.len(), 2);
    }

    #[tokio::test]
    async fn stale_external_edits_and_invalid_order_do_not_change_credentials() {
        let (dir, service) = fixture();
        let view = service
            .operation(
                &permit(),
                ModelOperation::Create(model("a", None, Some("original-key"))),
            )
            .await
            .unwrap();
        let secrets = read(dir.path().join(".env")).unwrap();
        write(
            dir.path().join("albert.toml"),
            format!(
                "{}\n# External edit\n",
                String::from_utf8(read(dir.path().join("albert.toml")).unwrap()).unwrap()
            ),
        )
        .unwrap();
        let error = service
            .operation(
                &permit(),
                ModelOperation::Update {
                    id: "a".into(),
                    input: model("a", Some(view.revision), Some("must-not-be-written")),
                },
            )
            .await
            .err()
            .unwrap();
        assert_eq!(error.status, StatusCode::PRECONDITION_FAILED);
        assert_eq!(read(dir.path().join(".env")).unwrap(), secrets);
        let current = service.list().await.unwrap();
        assert!(service
            .operation(
                &permit(),
                ModelOperation::Order(ModelOrder {
                    ids: vec!["a".into(), "a".into()],
                    revision: current.revision
                })
            )
            .await
            .is_err());
    }

    #[tokio::test]
    async fn linked_secret_files_and_pool_escape_are_rejected_without_host_path_errors() {
        let (dir, service) = fixture();
        let outside = tempdir().unwrap();
        let external = outside.path().join("external-secret");
        write(&external, "untouched").unwrap();
        symlink(&external, dir.path().join(".env")).unwrap();
        let error = service.list().await.err().unwrap();
        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert!(!error.message.contains(outside.path().to_str().unwrap()));
        assert_eq!(read(&external).unwrap(), b"untouched");
    }

    #[tokio::test]
    async fn malformed_pool_keeps_its_path_available_for_configuration_repair() {
        let (_dir, service) = fixture();
        let path = service.path().unwrap();
        create_dir_all(path.parent().unwrap()).unwrap();
        write(&path, "this is not a model pool").unwrap();
        assert_eq!(service.path().unwrap(), path);
        assert!(service.list().await.is_err());
        assert_eq!(read(path).unwrap(), b"this is not a model pool");
    }

    #[tokio::test]
    async fn legacy_model_keeps_its_provider_endpoint_and_key_reference_when_promoted() {
        let (dir, service) = fixture();
        write(dir.path().join("albert.toml"), "# Existing installation\nauth = \"api_key\"\nmodel = \"legacy-provider-model\"\nbase_url = \"http://127.0.0.1:18082/v1\"\nopenai_key_env = \"ALBERT_LEGACY_TEST_KEY\"\n").unwrap();
        write(
            dir.path().join(".env"),
            "ALBERT_LEGACY_TEST_KEY=legacy-private-value\n",
        )
        .unwrap();
        let legacy = service.list().await.unwrap();
        assert_eq!(legacy.models.len(), 1);
        assert_eq!(legacy.models[0].config.model, "legacy-provider-model");
        service
            .operation(
                &permit(),
                ModelOperation::Create(model("next", Some(legacy.revision), None)),
            )
            .await
            .unwrap();
        let persisted = PoolConfig::read(&service.path().unwrap()).unwrap();
        let preserved = &persisted.models[0];
        assert_eq!(preserved.model, "legacy-provider-model");
        assert_eq!(
            preserved.base_url.as_deref(),
            Some("http://127.0.0.1:18082/v1")
        );
        assert_eq!(
            preserved.api_key_env.as_deref(),
            Some("ALBERT_LEGACY_TEST_KEY")
        );
        assert!(
            String::from_utf8(read(dir.path().join("albert.toml")).unwrap())
                .unwrap()
                .contains("# Existing installation")
        );
    }
}
