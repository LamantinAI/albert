use albert_core::{
    db::error::DatabaseError, service::session::web::WebActor, sessions::error::SessionError,
};
use axum::http::StatusCode;
use octo_history::SessionId;

use super::{session_error, SessionHost};
use crate::api::{errors::ApiError, errors::ApiResult, host::HostResponse, models::EmptyObject};

impl SessionHost {
    pub(super) async fn delete(
        &self,
        actor: &WebActor,
        raw_id: &str,
        revision: i64,
    ) -> ApiResult<HostResponse> {
        let id = SessionId::parse(raw_id).map_err(|_| ApiError::invalid("Invalid Session ID."))?;
        let _guard = self.gate(&id).await;
        self.runtime
            .sessions
            .web_delete(actor, &id, revision, &self.runtime.base.code_workspace)
            .await
            .map_err(|error| {
                if matches!(
                    error,
                    SessionError::Database(DatabaseError::RevisionConflict)
                ) {
                    ApiError::new(
                        StatusCode::PRECONDITION_FAILED,
                        "revision_mismatch",
                        "Chat metadata changed. Reload before deleting it.",
                    )
                } else {
                    session_error(error)
                }
            })?;
        self.hub.changed(
            &id,
            actor.user_id.as_str(),
            &self.runtime.status().runtime_epoch,
            "ended",
            None,
        );
        self.hub.remove(&id);
        self.runtime.finish_session_deletion(&id).await.map_err(|_| ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE, "cleanup_pending",
            "The conversation was deleted, but attachment or task cleanup is still pending. Retry deletion to check completion.",
        ))?;
        Ok(HostResponse::Empty(EmptyObject {}))
    }
}

#[cfg(test)]
mod tests {
    use albert_core::media::save_input_file;
    use axum::http::StatusCode;
    use octo_history::SessionId;
    use sqlx::query;
    use std::{
        fs::create_dir_all, fs::rename, fs::write, sync::mpsc::sync_channel, time::Duration,
    };
    use tokio::{spawn, sync::oneshot, task::spawn_blocking, time::timeout};

    use super::super::fixture::fixture;
    use crate::api::host::{HostResponse, SessionOperation};

    #[tokio::test]
    async fn private_chat_deletion_is_revision_checked_idempotent_and_gone_for_all_views() {
        let (_dir, host, caller) = fixture().await;
        create_dir_all(&host.runtime.base.code_workspace).unwrap();
        let actor = host.actor(&caller).await.unwrap();
        let session = host
            .runtime
            .sessions
            .web_create(&actor, "Delete from both views", false)
            .await
            .unwrap();
        let wrong = host
            .operation(
                &caller,
                SessionOperation::Delete {
                    id: session.id.to_string(),
                    revision: session.revision + 1,
                },
            )
            .await
            .err()
            .unwrap();
        assert_eq!(wrong.status, StatusCode::PRECONDITION_FAILED);
        assert!(host.resource(&actor, session.id.as_str()).await.is_ok());
        assert!(matches!(
            host.operation(
                &caller,
                SessionOperation::Delete {
                    id: session.id.to_string(),
                    revision: session.revision
                }
            )
            .await
            .unwrap(),
            HostResponse::Empty(_)
        ));
        assert_eq!(
            host.resource(&actor, session.id.as_str())
                .await
                .err()
                .unwrap()
                .status,
            StatusCode::GONE
        );
        assert!(host
            .operation(
                &caller,
                SessionOperation::Delete {
                    id: session.id.to_string(),
                    revision: session.revision
                }
            )
            .await
            .is_ok());
        assert_eq!(
            query("SELECT * FROM albert_session_erasures")
                .fetch_all(host.runtime.sessions.database().pool())
                .await
                .unwrap()
                .len(),
            0
        );
    }
    #[tokio::test]
    async fn gone_does_not_mean_filesystem_cleanup_completed_and_delete_retry_resumes_it() {
        let (_dir, host, caller) = fixture().await;
        let root = &host.runtime.base.code_workspace;
        create_dir_all(root).unwrap();
        let actor = host.actor(&caller).await.unwrap();
        let session = host
            .runtime
            .sessions
            .web_create(&actor, "Pending cleanup", false)
            .await
            .unwrap();
        let asset = SessionId::new().to_string();
        query("INSERT INTO albert_media_assets(id,session_id,filename,content_type,size,sha256) VALUES (?,?,'fixture.bin','application/octet-stream',3,?)")
            .bind(&asset).bind(session.id.as_str()).bind("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad").execute(host.runtime.sessions.database().pool()).await.unwrap();
        let reserved = root.join(format!("media-{asset}"));
        create_dir_all(&reserved).unwrap();
        write(reserved.join("retain.txt"), "retain").unwrap();
        let result = host
            .operation(
                &caller,
                SessionOperation::Delete {
                    id: session.id.to_string(),
                    revision: session.revision,
                },
            )
            .await
            .err()
            .unwrap();
        assert_eq!(result.code, "cleanup_pending");
        assert_eq!(
            host.resource(&actor, session.id.as_str())
                .await
                .err()
                .unwrap()
                .status,
            StatusCode::GONE
        );
        assert!(reserved.join("retain.txt").exists());
        rename(&reserved, root.join("retained-files")).unwrap();
        assert!(host
            .operation(
                &caller,
                SessionOperation::Delete {
                    id: session.id.to_string(),
                    revision: session.revision
                }
            )
            .await
            .is_ok());
        assert!(root.join("retained-files/retain.txt").exists());
    }
    #[tokio::test]
    async fn canceled_unaccepted_preparation_is_owned_and_erased_without_persisting_attachment_bytes(
    ) {
        let (_dir, host, caller) = fixture().await;
        create_dir_all(&host.runtime.base.code_workspace).unwrap();
        let actor = host.actor(&caller).await.unwrap();
        let session = host
            .runtime
            .sessions
            .web_create(&actor, "Unaccepted attachment", false)
            .await
            .unwrap();
        let store = host.runtime.sessions.media();
        let asset = store
            .stage(
                &actor.account_id,
                b"fixture file".to_vec(),
                "application/octet-stream",
                "fixture.bin",
            )
            .await
            .unwrap();
        let prepared = host
            .prepare_media(
                &actor,
                Some(session.id.as_str()),
                "A canceled send",
                &[asset.id.clone()],
                false,
            )
            .await
            .unwrap();
        let relative = prepared.workspace_files[0].path.clone().unwrap();
        let file = host.runtime.base.code_workspace.join(&relative);
        assert!(file.exists());
        assert!(query("SELECT id FROM albert_media_assets")
            .fetch_all(host.runtime.sessions.database().pool())
            .await
            .unwrap()
            .is_empty());
        // Preparation did not change staging ownership, quotas or message acceptance.
        store
            .remove_staged(&actor.account_id, &asset.id)
            .await
            .unwrap();
        assert!(host
            .operation(
                &caller,
                SessionOperation::Delete {
                    id: session.id.to_string(),
                    revision: session.revision
                }
            )
            .await
            .is_ok());
        assert!(!file.exists());
        assert!(query("SELECT * FROM albert_session_workspace_copies")
            .fetch_all(host.runtime.sessions.database().pool())
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn deletion_waits_for_an_uncancelled_blocking_writer_before_erasing_its_copy() {
        let (_dir, host, caller) = fixture().await;
        let root = host.runtime.base.code_workspace.clone();
        create_dir_all(&root).unwrap();
        let actor = host.actor(&caller).await.unwrap();
        let session = host
            .runtime
            .sessions
            .web_create(&actor, "In-flight copy", false)
            .await
            .unwrap();
        let asset = SessionId::new().to_string();
        let preparation = host
            .runtime
            .sessions
            .journal_router()
            .handle(&session.id)
            .await
            .read_owned()
            .await;
        host.runtime
            .sessions
            .media()
            .register_workspace_copies(session.id.as_str(), &[asset.clone()], &root)
            .await
            .unwrap();
        let (ready_tx, ready_rx) = oneshot::channel();
        let (release_tx, release_rx) = sync_channel(1);
        let file = root.join(format!("media-{asset}"));
        let worker = spawn_blocking(move || {
            let _guard = preparation;
            let _ = ready_tx.send(());
            release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            save_input_file(&root, &asset, b"owned file").unwrap();
        });
        ready_rx.await.unwrap();
        let target = host.clone();
        let owner = caller.clone();
        let id = session.id.to_string();
        let mut deleting = spawn(async move {
            target
                .operation(
                    &owner,
                    SessionOperation::Delete {
                        id,
                        revision: session.revision,
                    },
                )
                .await
        });
        assert!(timeout(Duration::from_millis(50), &mut deleting)
            .await
            .is_err());
        release_tx.send(()).unwrap();
        worker.await.unwrap();
        timeout(Duration::from_secs(3), deleting)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(!file.exists());
    }
}
