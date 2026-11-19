//! Owner memory selection. File configuration is authoritative; a durable
//! checkpoint records first-run selection before the runtime may open a vault.
mod settings;
mod view;

use std::sync::Arc;

use albert_core::{db::repo::memory_setup, runtime::AgentRuntime};
use axum::http::StatusCode;
use tokio::{spawn, sync::Mutex};

use crate::{
    api::errors::ApiError, api::errors::ApiResult, api::host::HostResponse,
    api::memory::MemoryOperation, api::memory::MemoryView, api::principal::OwnerPermit,
    api::principal::Principal, service::control::ControlService,
};

#[derive(Clone)]
pub(crate) struct MemoryService {
    runtime: Arc<AgentRuntime>,
    gate: Arc<Mutex<()>>,
}

#[cfg(test)]
mod tests {
    use std::{fs::read_to_string, fs::write, sync::Arc};

    use albert_core::{config::Config, db::repo::memory_setup};
    use axum::http::StatusCode;
    use serde_json::{from_value, json, to_value};
    use sqlx::query;
    use tokio::{join, sync::Mutex};

    use super::MemoryService;
    use albert_core::db::repo::memory_setup::StartupPolicy;

    use crate::{api::memory::MemoryOperation, api::memory::MemoryWrite, web::testing::fixture};

    fn write_request(value: &str, backend: &str) -> MemoryWrite {
        from_value(json!({"revision": value, "config": {"backend": backend}})).unwrap()
    }

    #[tokio::test]
    async fn combined_candidate_is_validated_after_memory_is_overlaid() {
        let (_dir, runtime, _auth, owner) = fixture().await;
        write(
            &runtime.path,
            "[memory]\nbackend='mcp'\ntransport='http'\nurl='https://example.com/mcp'\n",
        )
        .unwrap();
        let service = MemoryService::new(runtime.clone(), Arc::new(Mutex::new(())));
        let view = service.view().await.unwrap();
        let document = service.control().document("albert").unwrap();
        let input=from_value(json!({"revision":view.revision,"config":{"backend":"embedded"},"document":{"revision":document.revision,"content":format!("{}\n[clouds.archive]\nurl='https://example.com'\ntoken_env='ARCHIVE_TOKEN'\n",document.content)}})).unwrap();
        service
            .operation(&owner, MemoryOperation::Put(input))
            .await
            .unwrap();
        let saved = Config::load_from(&runtime.path).unwrap();
        assert!(saved.clouds.contains_key("archive"));
        assert_eq!(
            to_value(service.view().await.unwrap().config).unwrap(),
            json!({"backend":"embedded"})
        );
    }

    #[tokio::test]
    async fn merged_save_preserves_both_drafts_and_checks_two_distinct_revisions() {
        let (_dir, runtime, _auth, owner) = fixture().await;
        write(
            &runtime.path,
            "# Keep comment\ntimezone='UTC'\n[memory]\nbackend='embedded'\n",
        )
        .unwrap();
        let service = MemoryService::new(runtime.clone(), Arc::new(Mutex::new(())));
        let view = service.view().await.unwrap();
        let document = service.control().document("albert").unwrap();
        let request = |revision: &str| {
            from_value(json!({"revision":view.revision,"config":{"backend":"mcp","transport":"http","url":"https://example.com/mcp"},"document":{"revision":revision,"content":document.content.replace("'UTC'", "'Europe/Paris'")}})).unwrap()
        };
        let error = service
            .operation(&owner, MemoryOperation::Put(request(&view.revision)))
            .await
            .err()
            .unwrap();
        assert_eq!(error.status, StatusCode::PRECONDITION_FAILED);
        assert_eq!(read_to_string(&runtime.path).unwrap(), document.content);
        let response = service
            .operation(&owner, MemoryOperation::Put(request(&document.revision)))
            .await
            .unwrap();
        let encoded = to_value(&response).unwrap();
        assert!(encoded["document"]["content"]
            .as_str()
            .unwrap()
            .contains("# Keep comment"));
        let saved = read_to_string(&runtime.path).unwrap();
        assert!(saved.contains("Europe/Paris") && saved.contains("https://example.com/mcp"));
        assert_ne!(service.view().await.unwrap().revision, view.revision);
    }

    #[tokio::test]
    async fn raw_memory_overlap_invalid_web_and_history_changes_have_no_side_effects() {
        let (dir, runtime, _auth, owner) = fixture().await;
        write(
            &runtime.path,
            "timezone='UTC'\n[memory]\nbackend='embedded'\n",
        )
        .unwrap();
        let service = MemoryService::new(runtime.clone(), Arc::new(Mutex::new(())));
        let view = service.view().await.unwrap();
        let document = service.control().document("albert").unwrap();
        for content in [
            "memory={backend='mcp',transport='http',url='https://example.com/mcp'}".to_owned(),
            "memory.backend='mcp'\nmemory.transport='http'\nmemory.url='https://example.com/mcp'"
                .to_owned(),
            format!("{}\n[web]\nbind='invalid'", document.content),
            format!("{}\n[history]\nbackend='sqlite:other.db'", document.content),
            format!(
                "{}\n[unused]\npassword='fixture-only-literal'",
                document.content
            ),
        ] {
            let input=from_value(json!({"revision":view.revision,"config":{"backend":"mcp","transport":"http","url":"https://example.com/mcp"},"token_value":"fixture-only-bearer","document":{"revision":document.revision,"content":content}})).unwrap();
            assert!(service
                .operation(&owner, MemoryOperation::Put(input))
                .await
                .is_err());
            assert_eq!(read_to_string(&runtime.path).unwrap(), document.content);
            assert!(!dir.path().join(".env").exists());
        }
        assert!(!memory_setup::read(runtime.sessions.database().pool())
            .await
            .unwrap()
            .selected());
    }

    #[tokio::test]
    async fn concurrent_memory_saves_and_external_secret_changes_cannot_overwrite_a_revision() {
        let (dir, runtime, _auth, owner) = fixture().await;
        write(&runtime.path, "timezone='UTC'\n").unwrap();
        let service = MemoryService::new(runtime.clone(), Arc::new(Mutex::new(())));
        let view = service.view().await.unwrap();
        write(
            dir.path().join(".env"),
            "UNRELATED_REFERENCE=fixture-only\n",
        )
        .unwrap();
        let stale = service
            .operation(
                &owner,
                MemoryOperation::Put(write_request(&view.revision, "embedded")),
            )
            .await;
        assert_eq!(stale.err().unwrap().status, StatusCode::PRECONDITION_FAILED);
        let view = service.view().await.unwrap();
        let (a, b) = join!(
            service.operation(
                &owner,
                MemoryOperation::Put(write_request(&view.revision, "embedded"))
            ),
            service.operation(
                &owner,
                MemoryOperation::Put(write_request(&view.revision, "embedded"))
            )
        );
        assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    }

    #[tokio::test]
    async fn a_failed_setup_checkpoint_reports_the_file_as_saved() {
        let (_dir, runtime, _auth, owner) = fixture().await;
        write(&runtime.path, "").unwrap();
        let service = MemoryService::new(runtime.clone(), Arc::new(Mutex::new(())));
        let view = service.view().await.unwrap();
        query("CREATE TRIGGER reject_setup BEFORE UPDATE ON albert_memory_setup BEGIN SELECT RAISE(ABORT,'fixture'); END").execute(runtime.sessions.database().pool()).await.unwrap();
        let error = service
            .operation(
                &owner,
                MemoryOperation::Put(write_request(&view.revision, "embedded")),
            )
            .await
            .err()
            .unwrap();
        assert_eq!(error.code, "memory_setup_incomplete");
        assert!(read_to_string(&runtime.path).unwrap().contains("[memory]"));
        assert!(!memory_setup::read(runtime.sessions.database().pool())
            .await
            .unwrap()
            .selected());
    }

    #[tokio::test]
    async fn stdio_credential_references_remain_editable_without_redacting_their_names() {
        let (_dir, runtime, _auth, owner) = fixture().await;
        write(&runtime.path,"timezone='UTC'\n[memory]\nbackend='mcp'\ntransport='stdio'\ncommand='kaeru'\n[memory.env]\nTOKEN='STORED_CREDENTIAL'\n").unwrap();
        let service = MemoryService::new(runtime.clone(), Arc::new(Mutex::new(())));
        let view = service.view().await.unwrap();
        let document = service.control().document("albert").unwrap();
        assert!(!document.redacted);
        let input=from_value(json!({"revision":view.revision,"config":view.config,"document":{"revision":document.revision,"content":document.content.replace("'UTC'","'Europe/Paris'")}})).unwrap();
        service
            .operation(&owner, MemoryOperation::Put(input))
            .await
            .unwrap();
        assert!(read_to_string(&runtime.path)
            .unwrap()
            .contains("STORED_CREDENTIAL"));
    }

    #[tokio::test]
    async fn choice_is_revisioned_preserves_other_settings_and_survives_reopen() {
        let (_dir, runtime, _auth, owner) = fixture().await;
        write(&runtime.path, "# Keep this comment\nmodel='fixture'\n").unwrap();
        let service = MemoryService::new(runtime.clone(), Arc::new(Mutex::new(())));
        let original = service.view().await.unwrap();
        assert!(original.selection_required);
        service
            .operation(
                &owner,
                MemoryOperation::Put(write_request(&original.revision, "embedded")),
            )
            .await
            .unwrap();
        let saved = service.view().await.unwrap();
        assert!(!saved.selection_required);
        assert_ne!(original.revision, saved.revision);
        let text = read_to_string(&runtime.path).unwrap();
        assert!(text.contains("# Keep this comment"));
        assert!(text.contains("model='fixture'"));
        assert!(text.contains("[memory]"));
        assert_eq!(
            memory_setup::initialize(
                runtime.sessions.database().pool(),
                StartupPolicy::BrowserOnboarding,
            )
            .await
            .unwrap()
            .state,
            "selected"
        );
        let error = service
            .operation(
                &owner,
                MemoryOperation::Put(write_request(&original.revision, "embedded")),
            )
            .await
            .err()
            .unwrap();
        assert_eq!(error.status, StatusCode::PRECONDITION_FAILED);
        assert_eq!(read_to_string(&runtime.path).unwrap(), text);
    }

    #[tokio::test]
    async fn bearer_is_write_only_and_transport_switch_removes_obsolete_settings() {
        let (dir, runtime, _auth, owner) = fixture().await;
        let service = MemoryService::new(runtime.clone(), Arc::new(Mutex::new(())));
        let first = service.view().await.unwrap();
        let input = from_value(json!({"revision":first.revision,"config":{"backend":"mcp","transport":"http","url":"https://example.com/mcp"},"token_value":"fixture-only-bearer"})).unwrap();
        service
            .operation(&owner, MemoryOperation::Put(input))
            .await
            .unwrap();
        let saved = service.view().await.unwrap();
        assert!(saved.token_configured);
        assert_eq!(
            saved.config.token_env.as_deref(),
            Some("ALBERT_MEMORY_MCP_TOKEN")
        );
        assert!(!to_value(&saved)
            .unwrap()
            .to_string()
            .contains("fixture-only-bearer"));
        assert!(!read_to_string(&runtime.path)
            .unwrap()
            .contains("fixture-only-bearer"));
        assert!(read_to_string(dir.path().join(".env"))
            .unwrap()
            .contains("fixture-only-bearer"));
        let input = from_value(json!({"revision":saved.revision,"config":{"backend":"mcp","transport":"stdio","command":"kaeru","args":["mcp"],"env":{"AUTH_TOKEN":"LOCAL_TOKEN"}}})).unwrap();
        service
            .operation(&owner, MemoryOperation::Put(input))
            .await
            .unwrap();
        let text = read_to_string(&runtime.path).unwrap();
        assert!(!text.contains("url"));
        assert!(!text.contains("token_env"));
        assert!(text.contains("LOCAL_TOKEN"));
        let current = service.view().await.unwrap();
        service
            .operation(
                &owner,
                MemoryOperation::Put(write_request(&current.revision, "embedded")),
            )
            .await
            .unwrap();
        let config = Config::load_from(&runtime.path).unwrap();
        assert_eq!(
            to_value(service.view().await.unwrap().config).unwrap(),
            json!({"backend":"embedded"})
        );
        assert!(config.secret("ALBERT_MEMORY_MCP_TOKEN").is_some());
    }

    #[tokio::test]
    async fn invalid_settings_and_cloud_conflicts_never_erase_existing_configuration() {
        let (dir, runtime, _auth, owner) = fixture().await;
        write(
            &runtime.path,
            "[clouds.archive]\nurl='https://example.com'\ntoken_env='ARCHIVE_TOKEN'\n",
        )
        .unwrap();
        let before = read_to_string(&runtime.path).unwrap();
        let service = MemoryService::new(runtime.clone(), Arc::new(Mutex::new(())));
        let revision = service.control().revision().unwrap();
        for config in [
            json!({"backend":"mcp","transport":"http","url":"https://example.com/mcp"}),
            json!({"backend":"mcp","transport":"http","url":"https://user:secret@example.com/mcp"}),
            json!({"backend":"mcp","transport":"stdio","command":"kaeru","env":{"TOKEN":"inline secret value"}}),
        ] {
            let input = from_value(
                json!({"revision":revision,"config":config,"token_value":"fixture-only-token"}),
            )
            .unwrap();
            assert!(service
                .operation(&owner, MemoryOperation::Put(input))
                .await
                .is_err());
            assert_eq!(read_to_string(&runtime.path).unwrap(), before);
            assert!(!dir.path().join(".env").exists());
        }
        assert!(!memory_setup::read(runtime.sessions.database().pool())
            .await
            .unwrap()
            .selected());
    }
}
impl MemoryService {
    pub fn new(runtime: Arc<AgentRuntime>, gate: Arc<Mutex<()>>) -> Self {
        Self { runtime, gate }
    }
    fn control(&self) -> ControlService {
        ControlService::new(self.runtime.clone(), self.gate.clone())
    }
    pub async fn operation(
        &self,
        caller: &Principal,
        operation: MemoryOperation,
    ) -> ApiResult<HostResponse> {
        let permit = caller.owner_permit()?;
        let service = self.clone();
        // Own the mutation lease/checkpoint even if the requesting browser closes.
        let view = spawn(async move { service.perform(permit, operation).await })
            .await
            .map_err(|_| ApiError::internal())??;
        Ok(HostResponse::Memory(view))
    }
    async fn perform(
        &self,
        permit: OwnerPermit,
        operation: MemoryOperation,
    ) -> ApiResult<MemoryView> {
        let gate = self.gate.lock().await;
        self.control().control_owner(&permit).await?;
        let mut include_document = false;
        match operation {
            MemoryOperation::Get => return self.view().await,
            MemoryOperation::Put(input) => {
                include_document = input.document.is_some();
                let _activation = self.runtime.memory_configuration_lease().await;
                self.revision(&input.revision)?;
                let pool = self.runtime.sessions.database().pool();
                let setup = memory_setup::read(pool)
                    .await
                    .map_err(|_| ApiError::internal())?;
                self.save(input)?;
                memory_setup::accept(pool, setup.revision).await.map_err(|_| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "memory_setup_incomplete", "Settings were saved, but memory setup could not be recorded. Review the saved settings and retry setup explicitly."))?;
            }
            MemoryOperation::Initialize(input) => {
                self.revision(&input.revision)?;
                if !memory_setup::read(self.runtime.sessions.database().pool())
                    .await
                    .map_err(|_| ApiError::internal())?
                    .selected()
                {
                    return Err(ApiError::conflict(
                        "Choose and save a memory backend first.",
                    ));
                }
            }
        }
        let committed_revision = self.control().revision()?;
        drop(gate);
        // An active agent retains its memory until an explicit process restart.
        if self.runtime.current().is_none() {
            let _ = self.runtime.activate().await;
        }
        let _gate = self.gate.lock().await;
        if self.control().revision()? != committed_revision {
            return Err(ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "memory_saved_configuration_changed", "The operation completed, but configuration changed before its status was confirmed. Review the saved settings before another operation."));
        }
        let mut view = self.view().await.map_err(|_| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "memory_status_unavailable", "The operation completed, but its current status could not be read. Review the saved settings before another operation."))?;
        if include_document {
            view.document = Some(self.control().document("albert")?);
        }
        Ok(view)
    }
    fn revision(&self, expected: &str) -> ApiResult<()> {
        if expected == self.control().revision()? {
            Ok(())
        } else {
            Err(ApiError::new(
                StatusCode::PRECONDITION_FAILED,
                "revision_changed",
                "Configuration changed. Reload before saving memory settings.",
            ))
        }
    }
}
