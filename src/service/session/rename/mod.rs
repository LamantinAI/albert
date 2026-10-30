//! Literal manual metadata edits. Native receipt and catalog CAS commit together.
use octo_connector_telegram::NativeFrame;
use octo_history::{AppendRequest, SessionKind, SessionState, SessionUpdate, Turn};
use serde_json::to_string;

use super::{route_key, SessionService};
use crate::{
    db::{error::DatabaseError, repo::route::find_in},
    messaging::contract::MESSAGE_MARKER,
    sessions::{
        error::{SessionError, SessionResult},
        native::request_alias,
        SessionContext,
    },
};

fn literal_title(title: &str) -> SessionResult<&str> {
    if title.chars().any(char::is_control) {
        return Err(SessionError::InvalidInput(
            "a chat name must be a single line without control characters",
        ));
    }
    let title = title.trim();
    if title.is_empty() || title.len() > 4096 {
        return Err(SessionError::InvalidInput(
            "use /rename <single-line name>, at most 4096 UTF-8 bytes",
        ));
    }
    Ok(title)
}
impl SessionService {
    pub(crate) async fn rename_private(
        &self,
        context: &SessionContext,
        title: &str,
        native: Option<&NativeFrame>,
    ) -> SessionResult<SessionContext> {
        context.require_private()?;
        let title = literal_title(title)?;
        let _lifecycle = self.router.lifecycle_gate().read_owned().await;
        let mut tx = self.history.begin_journal().await?;
        let current = self
            .catalog_session_in(&mut tx, context.session_id())
            .await?;
        context.check(&current)?;
        if current.kind == SessionKind::Temporary {
            return Err(SessionError::InvalidInput(
                "Save this temporary chat before renaming it with /rename.",
            ));
        }
        if current.state != SessionState::Active {
            return Err(SessionError::InvalidInput("this chat is archived"));
        }
        let route = find_in(&mut tx, &route_key(context)?)
            .await?
            .ok_or(SessionError::NotFound)?;
        if route.session_id != current.id.as_str()
            || Some(route.revision) != context.selector_revision()
        {
            return Err(DatabaseError::RevisionConflict.into());
        }
        let mut duplicate = false;
        if let Some(frame) = native {
            let alias = request_alias(frame);
            let record = Turn::assistant(format!(
                "{MESSAGE_MARKER}{}",
                to_string(&(
                    "albert.session_title.renamed.v1",
                    current.id.as_str(),
                    title
                ))
                .expect("title tuple serializes")
            ));
            let receipt = self
                .history
                .append_session_in(
                    &mut tx,
                    &context.journal(),
                    &AppendRequest {
                        namespace: alias.namespace,
                        idempotency_key: alias.idempotency_key,
                        digest: alias.digest,
                        turns: vec![record],
                    },
                )
                .await?;
            duplicate = receipt.duplicate;
        }
        let session = if duplicate {
            current
        } else {
            self.history
                .update_session_in(
                    &mut tx,
                    &current.id,
                    SessionUpdate {
                        expected_revision: context.session().revision,
                        title: Some(title.into()),
                        archived: None,
                    },
                )
                .await?
        };
        tx.commit().await.map_err(DatabaseError::from)?;
        let mut renamed = context.clone();
        renamed.session = session;
        Ok(renamed)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use octo_connector_telegram::NativeFrame;
    use octo_core::ConnectorId;
    use octo_history::{SessionStore, SessionUpdate, SqliteHistory};
    use tempfile::{tempdir, TempDir};

    use crate::{
        history::HistoryStore,
        service::session::SessionService,
        sessions::{
            settings::SessionSettings, Admission, AdmissionScope, ExternalIdentity, ReplyRoute,
            SessionContext,
        },
    };

    async fn fixture() -> (TempDir, SessionService, SessionContext) {
        let dir = tempdir().unwrap();
        let service = SessionService::from_history(
            Arc::new(
                SqliteHistory::open_retained(dir.path().join("history.db"))
                    .await
                    .unwrap(),
            ),
            SessionSettings::default(),
        )
        .await
        .unwrap();
        let context = service
            .admit(
                Admission::new(
                    ExternalIdentity::new("telegram", "7").unwrap(),
                    ReplyRoute::new("telegram", "7", None).unwrap(),
                    AdmissionScope::Private,
                    "host",
                )
                .unwrap(),
            )
            .await
            .unwrap();
        (dir, service, context)
    }
    fn frame(index: u32) -> NativeFrame {
        NativeFrame {
            connector: ConnectorId::new("telegram"),
            bot_id: 9,
            update_id: index,
            message_id: index as i32,
            date: 10 + index as i64,
            chat_id: 7,
            actor_id: Some(7),
            chat_type: "private".into(),
            command_text: Some("/rename literal".into()),
            digest: format!("digest-{index}"),
            media: false,
        }
    }
    #[tokio::test]
    async fn rename_is_literal_atomic_and_old_native_replay_cannot_undo_later_name() {
        let (_dir, service, context) = fixture().await;
        let first = service
            .rename_private(&context, "Untitled", Some(&frame(1)))
            .await
            .unwrap();
        assert_eq!(first.session().title, "Untitled");
        assert_eq!(first.selector_revision(), context.selector_revision());
        let second = service
            .rename_private(&first, "Исследование <SQLite>", Some(&frame(2)))
            .await
            .unwrap();
        let replay = service
            .rename_private(&second, "Untitled", Some(&frame(1)))
            .await
            .unwrap();
        assert_eq!(replay.session().title, "Исследование <SQLite>");
        assert_eq!(replay.session().revision, second.session().revision);
        let records = service.router.load(&context.root_history_key()).await;
        assert_eq!(records.len(), 2);
        assert!(records
            .iter()
            .all(|record| record.content.contains("albert.session_title.renamed.v1")));
        assert!(service.title_candidate(&second).await.unwrap().is_none());
    }
    #[tokio::test]
    async fn conflicting_revision_rolls_back_rename_audit_and_preserves_winner() {
        let (_dir, service, context) = fixture().await;
        service
            .history
            .update_session(
                context.session_id(),
                SessionUpdate {
                    expected_revision: context.session().revision,
                    title: Some("Concurrent manual name".into()),
                    archived: None,
                },
            )
            .await
            .unwrap();
        assert!(service
            .rename_private(&context, "Stale rename", Some(&frame(1)))
            .await
            .is_err());
        assert_eq!(
            service
                .get(&context, context.session_id())
                .await
                .unwrap()
                .title,
            "Concurrent manual name"
        );
        assert!(service
            .router
            .load(&context.root_history_key())
            .await
            .is_empty());
    }
    #[tokio::test]
    async fn validation_private_authority_selection_and_saved_temporary_lifecycle() {
        let (_dir, service, context) = fixture().await;
        for invalid in [
            "",
            "  ",
            "first\nsecond",
            "first\tsecond",
            &"Ж".repeat(2049),
        ] {
            assert!(service
                .rename_private(&context, invalid, None)
                .await
                .is_err());
        }
        let mut foreign = context.clone();
        foreign.user = None;
        assert!(service
            .rename_private(&foreign, "Denied", None)
            .await
            .is_err());
        let group = service
            .admit(
                Admission::new(
                    ExternalIdentity::new("telegram", "7").unwrap(),
                    ReplyRoute::new("telegram", "-42", None).unwrap(),
                    AdmissionScope::Group,
                    "host",
                )
                .unwrap(),
            )
            .await
            .unwrap();
        assert!(service
            .rename_private(&group, "Denied", None)
            .await
            .is_err());
        let temporary = service.start_temporary(&context, None).await.unwrap();
        assert!(service
            .rename_private(&temporary, "Private name", None)
            .await
            .unwrap_err()
            .to_string()
            .contains("Save"));
        let saved = service
            .save_temporary(&temporary, "rename-save", None)
            .await
            .unwrap();
        let renamed = service
            .rename_private(&saved, "Сохранённый чат", None)
            .await
            .unwrap();
        assert!(saved.is_live());
        assert_eq!(saved.selector_revision(), renamed.selector_revision());
        assert_eq!(renamed.session().title, "Сохранённый чат");
        let old = service.end_temporary(&renamed, None).await.unwrap().context;
        let next = service.create_private(&old, "Next").await.unwrap();
        assert!(service
            .rename_private(&old, "Stale selector", None)
            .await
            .is_err());
        assert_ne!(next.session_id(), old.session_id());
    }
}
