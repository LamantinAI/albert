//! Irreversible private chat erasure is distinct from ending a temporary view.
mod cleanup;
mod selectors;

use std::{env::current_dir, path::Path};

use octo_history::{SessionAccess, SessionId, SessionKind};

use super::{verify_actor, WebActor};
use crate::{
    db::entity::MarkerState, db::error::DatabaseError, db::repo::excursion::discard_in,
    db::repo::marker::find_in, db::repo::marker::set_owned, db::repo::session_erasure::erase_in,
    history::sessions::Backend, service::session::SessionService, sessions::error::SessionError,
    sessions::error::SessionResult,
};

impl SessionService {
    /// Commit the durable fence, content erasure and cleanup inventory atomically.
    /// The host completes runtime/filesystem cleanup before returning success.
    pub async fn web_delete(
        &self,
        actor: &WebActor,
        id: &SessionId,
        expected_revision: i64,
        workspace: &Path,
    ) -> SessionResult<()> {
        if expected_revision < 1 {
            return Err(SessionError::InvalidInput("invalid revision"));
        }
        let workspace = if workspace.is_absolute() {
            workspace.to_owned()
        } else {
            current_dir()
                .map_err(|_| SessionError::Unavailable)?
                .join(workspace)
        };
        let workspace = workspace
            .to_str()
            .ok_or(SessionError::InvalidInput("invalid workspace"))?;
        let _lifecycle = self.router.lifecycle_gate().write_owned().await;
        let mut backend = self.router.handle(id).await.write_owned().await;
        let mut tx = self.history.begin_journal().await?;
        verify_actor(&mut tx, actor).await?;
        let marker = find_in(&mut tx, id.as_str()).await?;
        if let Some(marker) = &marker {
            if marker.owner_user_id.as_deref() != Some(actor.user_id.as_str()) {
                return Err(SessionError::Denied);
            }
            if marker.state == MarkerState::Closed {
                self.leases.close(id)?;
                *backend = Backend::Closed;
                tx.commit().await.map_err(DatabaseError::from)?;
                return Ok(());
            }
        }
        let session = self.catalog_session_in(&mut tx, id).await?;
        if session.access
            != (SessionAccess::Private {
                owner: actor.user_id.as_str().into(),
            })
            || session.kind != SessionKind::Permanent
        {
            return Err(SessionError::Denied);
        }
        if session.revision != expected_revision {
            return Err(DatabaseError::RevisionConflict.into());
        }
        set_owned(
            &mut tx,
            id.as_str(),
            marker
                .as_ref()
                .map(|m| m.runtime_epoch.as_str())
                .unwrap_or(&self.runtime_epoch),
            MarkerState::Closed,
            marker.as_ref().map(|m| m.revision),
            actor.user_id.as_str(),
        )
        .await?;
        self.rotate_deleted_selectors(&mut tx, &session, actor.user_id.as_str())
            .await?;
        discard_in(&mut tx, id.as_str()).await?;
        erase_in(&mut tx, id.as_str(), workspace).await?;
        self.history
            .delete_session_in(&mut tx, id, expected_revision)
            .await?;
        // The backend writer already excludes appends. Fence shared authority
        // only after every fallible erasure statement, immediately before commit.
        self.leases.close(id)?;
        *backend = Backend::Closed;
        tx.commit().await.map_err(DatabaseError::from)?;
        self.credentials.cancel_session(id.as_str());
        self.confirmations.cancel_session(id.as_str());
        self.media().discard_session(id.as_str()).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use octo_history::{
        AppendRequest, CreateSession, HistoryError, HistoryStore, SessionAccess, SessionId,
        SessionJournal, SessionOrigin, SessionStore, Turn,
    };
    use sqlx::{query, query_as, query_scalar};
    use std::{fs::create_dir, fs::write};

    use super::super::tests::fixture;
    use crate::{
        db::entity::PrivateRouteKey, db::repo::route::set, media::discard_input_file,
        media::encode_prepared, media::WorkspaceFile, service::session::SessionService,
        sessions::error::SessionError, sessions::settings::SessionSettings,
    };

    #[tokio::test]
    async fn deletion_erases_content_and_attachments_but_retains_owned_restart_and_replay_fences() {
        let (dir, service, actor, foreign) = fixture().await;
        let workspace = dir.path().join("workspace");
        create_dir(&workspace).unwrap();
        let session = service.web_create(&actor, "Private", false).await.unwrap();
        let context = service.web_context(&actor, &session.id).await.unwrap();
        let request = AppendRequest {
            namespace: "native-delete".into(),
            idempotency_key: "input".into(),
            digest: "digest".into(),
            turns: vec![Turn::user("Erase this conversation")],
        };
        service
            .journal_router()
            .append_journal(&context.root_history_key(), &request)
            .await
            .unwrap();
        service
            .journal_router()
            .save_compact(&context.root_history_key(), None, 1, "Erase this summary")
            .await
            .unwrap();
        let asset = SessionId::new().to_string();
        query("INSERT INTO albert_media_assets(id,session_id,filename,content_type,size,bytes) VALUES (?,?,'fixture.bin','application/octet-stream',3,?)")
            .bind(&asset).bind(session.id.as_str()).bind(b"abc".as_slice()).execute(service.database.pool()).await.unwrap();
        write(workspace.join(format!("media-{asset}")), "abc").unwrap();
        service
            .web_delete(&actor, &session.id, session.revision, &workspace)
            .await
            .unwrap();
        assert!(!context.is_live());
        assert!(service.web_context(&actor, &session.id).await.is_err());
        for table in ["turns", "compacts", "albert_media_assets"] {
            assert_eq!(
                query_scalar::<_, i64>(&format!("SELECT count(*) FROM {table}"))
                    .fetch_one(service.database.pool())
                    .await
                    .unwrap(),
                0
            );
        }
        assert!(matches!(
            service
                .journal_router()
                .find_request("native-delete", "input")
                .await,
            Err(HistoryError::RequestUnbound)
        ));
        assert!(service
            .journal_router()
            .append_journal(&context.root_history_key(), &request)
            .await
            .is_err());
        assert!(matches!(
            service
                .web_delete(&foreign, &session.id, session.revision, &workspace)
                .await,
            Err(SessionError::Denied)
        ));
        // Simulate a crash after SQL commit: inventory retains the exact original
        // workspace and asset identity until filesystem cleanup is acknowledged.
        let reopened =
            SessionService::from_history(service.history().clone(), SessionSettings::default())
                .await
                .unwrap();
        let cleanup = reopened.erasure_files(&session.id).await.unwrap().unwrap();
        assert_eq!(cleanup.workspace, workspace);
        assert_eq!(cleanup.assets, vec![(asset.clone(), workspace.clone())]);
        assert!(reopened.web_context(&actor, &session.id).await.is_err());
        assert!(matches!(
            reopened
                .journal_router()
                .find_request("native-delete", "input")
                .await,
            Err(HistoryError::RequestUnbound)
        ));
        discard_input_file(&cleanup.workspace, &asset).unwrap();
        // A crash after unlink is retryable as well.
        discard_input_file(&cleanup.workspace, &asset).unwrap();
        reopened.acknowledge_erasure(&session.id).await.unwrap();
        reopened
            .web_delete(&actor, &session.id, session.revision, &workspace)
            .await
            .unwrap();
        assert!(reopened.pending_erasures().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn deleting_a_default_rotates_selectors_but_preserves_active_excursion_fences() {
        let (dir, service, actor, _) = fixture().await;
        let origin = SessionOrigin {
            connector: "telegram".into(),
            channel: "private-chat".into(),
        };
        let default = service
            .history
            .ensure_default_session(&origin, CreateSession::private(actor.user_id.as_str()))
            .await
            .unwrap();
        let temp = service.web_create(&actor, "Temporary", true).await.unwrap();
        let key = PrivateRouteKey {
            connector_id: origin.connector.clone(),
            channel_id: origin.channel.clone(),
            user_id: actor.user_id.as_str().into(),
        };
        let mut tx = service.history.begin_journal().await.unwrap();
        set(
            &mut tx,
            &key,
            temp.id.as_str(),
            Some(default.id.as_str()),
            None,
        )
        .await
        .unwrap();
        query("INSERT INTO albert_temporary_excursions(connector_id,bot_id,chat_id,actor_id,user_id,session_id,return_session_id,scope_epoch,start_date,start_update_id,start_message_id,disposition,revision) VALUES ('telegram','bot','private-chat','actor',?,?,?,'epoch',1,1,1,'active',1)")
            .bind(actor.user_id.as_str()).bind(temp.id.as_str()).bind(default.id.as_str()).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        service
            .web_delete(&actor, &default.id, default.revision, dir.path())
            .await
            .unwrap();
        let replacement = service
            .history
            .get_default_session(&origin)
            .await
            .unwrap()
            .unwrap();
        assert_ne!(replacement.id, default.id);
        assert_eq!(replacement.title, "Standard");
        assert_eq!(replacement.access, default.access);
        assert!(service
            .history
            .load_session(&SessionJournal::Root(replacement.id.clone()))
            .await
            .unwrap()
            .is_empty());
        let (current, back, revision): (String, String, i64) = query_as("SELECT session_id,return_session_id,revision FROM albert_private_session_routes WHERE user_id=?")
            .bind(actor.user_id.as_str()).fetch_one(service.database.pool()).await.unwrap();
        assert_eq!(current, temp.id.as_str());
        assert_eq!(back, replacement.id.as_str());
        assert_eq!(revision, 2);
        let (historical_return, disposition): (String,String) = query_as("SELECT return_session_id,disposition FROM albert_temporary_excursions WHERE session_id=?")
            .bind(temp.id.as_str()).fetch_one(service.database.pool()).await.unwrap();
        assert_eq!(historical_return, default.id.as_str());
        assert_eq!(disposition, "active");
        assert!(service.web_context(&actor, &temp.id).await.is_ok());
    }

    #[tokio::test]
    async fn foreign_group_stale_revision_and_storage_failure_do_not_erase_or_fence_content() {
        let (dir, service, actor, foreign) = fixture().await;
        let session = service
            .web_create(&actor, "Retained on failure", false)
            .await
            .unwrap();
        let context = service.web_context(&actor, &session.id).await.unwrap();
        assert!(service
            .web_delete(&foreign, &session.id, session.revision, dir.path())
            .await
            .is_err());
        assert!(service
            .web_delete(&actor, &session.id, session.revision + 1, dir.path())
            .await
            .is_err());
        let mut create = CreateSession::private("unused");
        create.access = SessionAccess::GroupBound {
            connector: "telegram".into(),
            channel: "group".into(),
        };
        let group = service.history.create_session(create).await.unwrap();
        assert!(service
            .web_delete(&actor, &group.id, group.revision, dir.path())
            .await
            .is_err());
        query("CREATE TRIGGER erasure_failure BEFORE DELETE ON history_sessions BEGIN SELECT RAISE(ABORT,'fixture failure'); END").execute(service.database.pool()).await.unwrap();
        assert!(service
            .web_delete(&actor, &session.id, session.revision, dir.path())
            .await
            .is_err());
        assert!(context.is_live());
        assert!(service.web_context(&actor, &session.id).await.is_ok());
        assert!(service.pending_erasures().await.unwrap().is_empty());
        query("DROP TRIGGER erasure_failure")
            .execute(service.database.pool())
            .await
            .unwrap();
        service
            .web_delete(&actor, &session.id, session.revision, dir.path())
            .await
            .unwrap();
    }
    #[tokio::test]
    async fn first_start_binds_prepared_copy_root_atomically_and_deleted_start_ids_never_create_new_chats(
    ) {
        let (dir, service, actor, _) = fixture().await;
        let original = dir.path().join("original");
        create_dir(&original).unwrap();
        let asset = service
            .media()
            .stage(
                &actor.account_id,
                b"abc".to_vec(),
                "application/octet-stream",
                "fixture.bin",
            )
            .await
            .unwrap();
        let path = format!("media-{}", asset.id);
        write(original.join(&path), "abc").unwrap();
        let input = AppendRequest {
            namespace: "first-file".into(),
            idempotency_key: "first-message".into(),
            digest: "digest".into(),
            turns: vec![Turn::user(encode_prepared(
                "Review this".into(),
                vec![asset.clone()],
                vec![],
                vec![WorkspaceFile {
                    asset_id: asset.id.clone(),
                    path: Some(path),
                }],
            ))],
        };
        let row = service
            .web_start_create_prepared(
                &actor,
                "request",
                "body",
                false,
                input.clone(),
                Some(&original),
            )
            .await
            .unwrap();
        let id = SessionId::parse(&row.session).unwrap();
        let metadata = service.web_context(&actor, &id).await.unwrap();
        service
            .web_delete(&actor, &id, metadata.session().revision, dir.path())
            .await
            .unwrap();
        let cleanup = service.erasure_files(&id).await.unwrap().unwrap();
        assert_eq!(cleanup.assets, vec![(asset.id.clone(), original)]);
        let replay = service
            .web_start_create_prepared(&actor, "request", "body", false, input, Some(dir.path()))
            .await
            .unwrap();
        assert_eq!(replay.session, row.session);
        assert_eq!(replay.phase, "expired");
        assert_eq!(
            query_scalar::<_, i64>("SELECT count(*) FROM history_sessions")
                .fetch_one(service.database.pool())
                .await
                .unwrap(),
            0
        );
    }
}
