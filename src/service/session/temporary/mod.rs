//! Temporary content stays in the shared retained RAM backend. Only identity,
//! routing and lifecycle markers become durable before an explicit Save.
mod control;
mod end;
mod save;
mod start;

use octo_history::SessionId;

use crate::sessions::SessionContext;

pub(crate) struct EndResult {
    pub session_id: SessionId,
    pub discarded: bool,
    pub ram_discarded: bool,
    pub context: SessionContext,
    pub cleanup_scope: String,
    pub scope_epoch: String,
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use octo_history::{
        AppendRequest, HistoryStore, PageRequest, SessionId, SessionStore, SqliteHistory, Turn,
    };
    use sqlx::query;
    use tempfile::{tempdir, TempDir};

    use crate::{
        history::sessions::Backend,
        service::session::SessionService,
        sessions::{
            error::SessionError, settings::SessionSettings, Admission, AdmissionScope,
            ExternalIdentity, ReplyRoute, SessionContext,
        },
    };

    fn admission() -> Admission {
        Admission::new(
            ExternalIdentity::new("telegram", "7").unwrap(),
            ReplyRoute::new("telegram", "7", None).unwrap(),
            AdmissionScope::Private,
            "host",
        )
        .unwrap()
    }

    async fn fixture() -> (TempDir, SessionService, SessionContext) {
        let dir = tempdir().unwrap();
        let history = Arc::new(
            SqliteHistory::open_retained(dir.path().join("history.db"))
                .await
                .unwrap(),
        );
        let service = SessionService::from_history(history, SessionSettings::default())
            .await
            .unwrap();
        let context = service.admit(admission()).await.unwrap();
        (dir, service, context)
    }

    #[tokio::test]
    async fn save_moves_full_journal_and_receipts_once_without_merging_the_original() {
        let (_dir, service, original) = fixture().await;
        let tmp = service.start_temporary(&original, None).await.unwrap();
        let key = tmp.root_history_key();
        let request = AppendRequest {
            namespace: "test".into(),
            idempotency_key: "request".into(),
            digest: "body".into(),
            turns: vec![Turn::user("secret"), Turn::assistant("answer")],
        };
        let before = service.router.append_journal(&key, &request).await.unwrap();
        let child_key = format!("{key}/child/opaque/run:id");
        service
            .router
            .append(&child_key, &[Turn::user("child secret")])
            .await
            .unwrap();
        assert!(service
            .history
            .get_session(tmp.session_id())
            .await
            .unwrap()
            .is_none());
        assert!(service
            .history
            .journal_page(&key, PageRequest::default())
            .await
            .unwrap()
            .records
            .is_empty());
        let saved = service.save_temporary(&tmp, "save", None).await.unwrap();
        assert_eq!(saved.session_id(), tmp.session_id());
        assert_eq!(saved.scope_epoch(), tmp.scope_epoch());
        assert!(tmp.is_live());
        assert_eq!(
            service.router.strict_load(&child_key).await.unwrap().len(),
            1
        );
        assert_eq!(service.router.strict_load(&key).await.unwrap().len(), 2);
        let replay = service.router.append_journal(&key, &request).await.unwrap();
        assert!(replay.duplicate);
        assert_eq!(before.record_ids, replay.record_ids);
        assert!(service
            .router
            .strict_load(&original.root_history_key())
            .await
            .unwrap()
            .is_empty());
        assert!(service
            .router
            .temporary()
            .get_session(tmp.session_id())
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            service
                .save_temporary(&saved, "different-retry-key", None)
                .await
                .unwrap()
                .session_id(),
            tmp.session_id()
        );
        let ended = service.end_temporary(&saved, None).await.unwrap();
        assert!(!ended.discarded);
        assert!(tmp.is_live());
        assert_eq!(ended.context.session_id(), original.session_id());
        service
            .router
            .append(&key, &[Turn::assistant("late permanent answer")])
            .await
            .unwrap();
        assert_eq!(service.router.strict_load(&key).await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn end_fences_root_and_children_and_leaves_the_original_untouched() {
        let (_dir, service, original) = fixture().await;
        service
            .router
            .append(&original.root_history_key(), &[Turn::user("original")])
            .await
            .unwrap();
        let tmp = service.start_temporary(&original, None).await.unwrap();
        service
            .router
            .append(&tmp.root_history_key(), &[Turn::user("private")])
            .await
            .unwrap();
        let child_key = format!("{}/child/run", tmp.root_history_key());
        service
            .router
            .append(&child_key, &[Turn::assistant("child private")])
            .await
            .unwrap();
        let ended = service.end_temporary(&tmp, None).await.unwrap();
        assert!(ended.discarded && ended.ram_discarded);
        assert!(!tmp.is_live());
        assert!(service
            .router
            .append(&tmp.root_history_key(), &[Turn::assistant("late")])
            .await
            .is_err());
        assert!(service.router.strict_load(&child_key).await.is_err());
        assert!(service
            .history
            .get_session(tmp.session_id())
            .await
            .unwrap()
            .is_none());
        assert!(service
            .router
            .temporary()
            .get_session(tmp.session_id())
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            service
                .router
                .strict_load(&original.root_history_key())
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            service.admit(admission()).await.unwrap().session_id(),
            original.session_id()
        );
    }

    #[tokio::test]
    async fn failed_save_preserves_ram_and_does_not_claim_success() {
        let (_dir, service, original) = fixture().await;
        let tmp = service.start_temporary(&original, None).await.unwrap();
        service
            .router
            .append(&tmp.root_history_key(), &[Turn::user("still here")])
            .await
            .unwrap();
        query("CREATE TRIGGER refuse_save BEFORE INSERT ON history_sessions BEGIN SELECT RAISE(ABORT, 'test failure'); END")
            .execute(service.database.pool()).await.unwrap();
        assert!(service.save_temporary(&tmp, "save", None).await.is_err());
        assert_eq!(
            *service.router.handle(tmp.session_id()).await.read().await,
            Backend::Temporary
        );
        assert!(tmp.is_live());
        assert_eq!(
            service
                .router
                .strict_load(&tmp.root_history_key())
                .await
                .unwrap()
                .len(),
            1
        );
        assert!(service
            .history
            .get_session(tmp.session_id())
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn restart_expires_unsaved_chat_without_falling_back_and_explicit_end_recovers() {
        let (_dir, service, original) = fixture().await;
        let tmp = service.start_temporary(&original, None).await.unwrap();
        service
            .router
            .append(&tmp.root_history_key(), &[Turn::user("not durable")])
            .await
            .unwrap();
        let restarted =
            SessionService::from_history(service.history.clone(), SessionSettings::default())
                .await
                .unwrap();
        assert!(matches!(
            restarted.admit(admission()).await,
            Err(SessionError::Expired)
        ));
        assert!(restarted
            .router
            .temporary()
            .get_session(tmp.session_id())
            .await
            .unwrap()
            .is_none());
        let control = restarted.control_context(admission()).await.unwrap();
        let ended = restarted.end_temporary(&control, None).await.unwrap();
        assert!(ended.discarded && ended.ram_discarded);
        assert_eq!(ended.context.session_id(), original.session_id());
        assert_eq!(
            restarted.admit(admission()).await.unwrap().session_id(),
            original.session_id()
        );
    }

    #[tokio::test]
    async fn saved_chat_survives_restart_with_the_same_scope() {
        let (_dir, service, original) = fixture().await;
        let tmp = service.start_temporary(&original, None).await.unwrap();
        service
            .router
            .append(&tmp.root_history_key(), &[Turn::user("durable")])
            .await
            .unwrap();
        let saved = service.save_temporary(&tmp, "save", None).await.unwrap();
        let restarted =
            SessionService::from_history(service.history.clone(), SessionSettings::default())
                .await
                .unwrap();
        let readmitted = restarted.admit(admission()).await.unwrap();
        assert_eq!(readmitted.session_id(), saved.session_id());
        assert_eq!(readmitted.scope_epoch(), saved.scope_epoch());
        assert!(readmitted.is_live());
        assert_eq!(
            restarted
                .router
                .strict_load(&readmitted.root_history_key())
                .await
                .unwrap()
                .len(),
            1
        );
        assert!(restarted
            .end_temporary(&readmitted, None)
            .await
            .unwrap()
            .context
            .is_live());
    }

    #[tokio::test]
    async fn navigation_cannot_abandon_an_active_excursion_and_repeated_tmp_keeps_content() {
        let (_dir, service, original) = fixture().await;
        let tmp = service.start_temporary(&original, None).await.unwrap();
        service
            .router
            .append(&tmp.root_history_key(), &[Turn::user("keep")])
            .await
            .unwrap();
        let repeated = service.start_temporary(&tmp, None).await.unwrap();
        assert_eq!(tmp.session_id(), repeated.session_id());
        assert!(service.create_private(&tmp, "orphan").await.is_err());
        assert!(service
            .open_private(
                &tmp,
                original.session_id(),
                tmp.selector_revision().unwrap()
            )
            .await
            .is_err());
        assert_eq!(
            service
                .router
                .strict_load(&tmp.root_history_key())
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn closed_or_foreign_uuid_is_not_an_access_permit() {
        let (_dir, service, original) = fixture().await;
        let tmp = service.start_temporary(&original, None).await.unwrap();
        let other = service
            .admit(
                Admission::new(
                    ExternalIdentity::new("telegram", "8").unwrap(),
                    ReplyRoute::new("telegram", "8", None).unwrap(),
                    AdmissionScope::Private,
                    "host",
                )
                .unwrap(),
            )
            .await
            .unwrap();
        assert!(matches!(
            service.save_temporary(&other, "save", None).await,
            Err(SessionError::InvalidInput(_))
        ));
        assert!(service
            .open_private(&other, tmp.session_id(), other.selector_revision().unwrap())
            .await
            .is_err());
        service.end_temporary(&tmp, None).await.unwrap();
        assert!(service.get(&original, tmp.session_id()).await.is_err());
        assert!(service.get(&original, &SessionId::new()).await.is_err());
    }
}
