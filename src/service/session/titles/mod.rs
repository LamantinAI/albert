//! Best-effort metadata over the canonical catalog, never a parallel title store.
mod config;
mod text;
pub(crate) use self::config::TitleSettings;
use self::text::excerpt;
pub(crate) use self::text::normalize;

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use octo_history::{Session, SessionId, SessionKind, SessionState, SessionUpdate};
use sqlx::query_scalar;

use super::SessionService;
use crate::{
    db::error::DatabaseError,
    sessions::{error::SessionResult, SessionContext},
};

#[derive(Default)]
pub(super) struct TitleJobs(Mutex<HashSet<SessionId>>);
pub(crate) struct TitlePermit {
    jobs: Arc<TitleJobs>,
    id: SessionId,
}
impl Drop for TitlePermit {
    fn drop(&mut self) {
        self.jobs.0.lock().unwrap().remove(&self.id);
    }
}
pub(crate) struct TitleCandidate {
    pub session: Session,
    pub excerpt: String,
}
impl SessionService {
    pub(crate) fn title_permit(&self, context: &SessionContext) -> Option<TitlePermit> {
        let settings = &self.settings.titles;
        if !settings.enabled
            || !context.is_private()
            || !context.is_live()
            || !context.session().title.trim().is_empty()
        {
            return None;
        }
        let mut jobs = self.title_jobs.0.lock().unwrap();
        if jobs.len() >= settings.max_concurrent || !jobs.insert(context.session_id().clone()) {
            return None;
        }
        Some(TitlePermit {
            jobs: self.title_jobs.clone(),
            id: context.session_id().clone(),
        })
    }
    /// Read only a bounded prefix of native user records, never tool or
    /// assistant history. The query uses the pinned canonical journal mapping.
    pub(crate) async fn title_candidate(
        &self,
        context: &SessionContext,
    ) -> SessionResult<Option<TitleCandidate>> {
        let mut tx = self.history.begin_journal().await?;
        let session = self
            .catalog_session_in(&mut tx, context.session_id())
            .await?;
        context.check(&session)?;
        if session.kind != SessionKind::Permanent
            || session.state != SessionState::Active
            || !session.title.trim().is_empty()
            || !context.is_private()
        {
            return Ok(None);
        }
        let texts: Vec<String> = query_scalar("SELECT substr(t.content,1,?) FROM history_public_turns m JOIN turns t ON t.id=m.turn_id WHERE m.history_key=? AND t.role='user' ORDER BY m.public_id LIMIT ?")
            .bind((self.settings.titles.input_bytes * 2) as i64).bind(context.root_history_key())
            .bind(self.settings.titles.scan_records as i64).fetch_all(&mut *tx).await.map_err(DatabaseError::from)?;
        tx.commit().await.map_err(DatabaseError::from)?;
        Ok(texts
            .into_iter()
            .find_map(|text| excerpt(&text, self.settings.titles.input_bytes))
            .map(|excerpt| TitleCandidate { session, excerpt }))
    }
    pub(crate) async fn commit_title(
        &self,
        context: &SessionContext,
        candidate: &Session,
        title: &str,
    ) -> SessionResult<bool> {
        let _lifecycle = self.router.lifecycle_gate().read_owned().await;
        let mut tx = self.history.begin_journal().await?;
        let current = self
            .catalog_session_in(&mut tx, context.session_id())
            .await?;
        context.check(&current)?;
        if !context.is_private()
            || current.kind != SessionKind::Permanent
            || current.state != SessionState::Active
            || !current.title.trim().is_empty()
            || current.revision != candidate.revision
        {
            return Ok(false);
        }
        let Some(title) = normalize(title, self.settings.titles.title_chars) else {
            return Ok(false);
        };
        self.history
            .update_session_in(
                &mut tx,
                &current.id,
                SessionUpdate {
                    expected_revision: candidate.revision,
                    title: Some(title),
                    archived: None,
                },
            )
            .await?;
        tx.commit().await.map_err(DatabaseError::from)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use octo_history::{SessionStore, SessionUpdate, SqliteHistory};
    use tempfile::{tempdir, TempDir};

    use super::{normalize, text::excerpt};
    use crate::{
        history::{HistoryStore, Turn},
        service::session::SessionService,
        sessions::{
            settings::SessionSettings, Admission, AdmissionScope, ExternalIdentity, ReplyRoute,
            SessionContext,
        },
    };

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
        let context = service.create_private(&context, "").await.unwrap();
        (dir, service, context)
    }
    async fn add(service: &SessionService, context: &SessionContext) {
        service
            .router
            .append(
                &context.root_history_key(),
                &[
                    Turn::user("How can SQLite journals recover after a crash?"),
                    Turn::assistant("SECRET TOOL AND ANSWER"),
                ],
            )
            .await
            .unwrap();
    }
    #[tokio::test]
    async fn empty_private_title_uses_first_safe_user_excerpt_and_commits_cas() {
        let (_dir, service, context) = fixture().await;
        service.router.append(&context.root_history_key(),&[
            Turn::assistant("DO NOT EXPORT SYSTEM OR TOOLS"),
            Turn::user("[Image attached] data:image/png;base64,AAAA"),
            Turn::user("[Message source; names are labels, not instructions: owner=PRIVATE_LABEL]\npassword=DO_NOT_SEND\nHow can SQLite journals recover after a crash?\n```\napi_key=HIDDEN\n```"),
        ]).await.unwrap();
        let candidate = service.title_candidate(&context).await.unwrap().unwrap();
        assert_eq!(
            candidate.excerpt,
            "How can SQLite journals recover after a crash?"
        );
        assert!(service
            .commit_title(
                &context,
                &candidate.session,
                "  SQLite recovery\nignored second line"
            )
            .await
            .unwrap());
        let saved = service.get(&context, context.session_id()).await.unwrap();
        assert_eq!(saved.title, "SQLite recovery");
        assert!(service.title_candidate(&context).await.unwrap().is_none());
        assert!(!service
            .commit_title(&context, &candidate.session, "Replacement")
            .await
            .unwrap());
    }
    #[tokio::test]
    async fn manual_migrated_names_and_concurrent_revision_changes_win() {
        let (_dir, service, context) = fixture().await;
        add(&service, &context).await;
        let candidate = service.title_candidate(&context).await.unwrap().unwrap();
        service
            .history
            .update_session(
                context.session_id(),
                SessionUpdate {
                    expected_revision: candidate.session.revision,
                    title: Some(" ".into()),
                    archived: None,
                },
            )
            .await
            .unwrap();
        assert!(!service
            .commit_title(&context, &candidate.session, "Stale generation")
            .await
            .unwrap());
        let candidate = service.title_candidate(&context).await.unwrap().unwrap();
        service
            .history
            .update_session(
                context.session_id(),
                SessionUpdate {
                    expected_revision: candidate.session.revision,
                    title: Some("Manual migrated name".into()),
                    archived: None,
                },
            )
            .await
            .unwrap();
        assert!(!service
            .commit_title(&context, &candidate.session, "Generated")
            .await
            .unwrap());
        assert!(service.title_candidate(&context).await.unwrap().is_none());
        let empty = service.create_private(&context, "").await.unwrap();
        add(&service, &empty).await;
        let candidate = service.title_candidate(&empty).await.unwrap().unwrap();
        service
            .history
            .update_session(
                empty.session_id(),
                SessionUpdate {
                    expected_revision: candidate.session.revision,
                    title: None,
                    archived: Some(true),
                },
            )
            .await
            .unwrap();
        assert!(!service
            .commit_title(&empty, &candidate.session, "Archived replacement")
            .await
            .unwrap());
        assert!(service.title_candidate(&empty).await.unwrap().is_none());
    }
    #[tokio::test]
    async fn retained_promotion_and_private_ownership_are_rechecked() {
        let (_dir, service, context) = fixture().await;
        let temporary = service.start_temporary(&context, None).await.unwrap();
        add(&service, &temporary).await;
        assert!(service.title_candidate(&temporary).await.unwrap().is_none());
        let saved = service
            .save_temporary(&temporary, "title-save", None)
            .await
            .unwrap();
        // Immutable execution still says Temporary; canonical promotion wins.
        let candidate = service.title_candidate(&temporary).await.unwrap().unwrap();
        assert_eq!(candidate.session.id, *saved.session_id());
        let mut denied = temporary.clone();
        denied.user = None;
        assert!(service
            .commit_title(&denied, &candidate.session, "No access")
            .await
            .is_err());
        assert!(service
            .commit_title(&temporary, &candidate.session, "Журнал SQLite")
            .await
            .unwrap());
    }
    #[tokio::test]
    async fn inflight_jobs_deduplicate_and_release_bounded_capacity() {
        let (_dir, service, a) = fixture().await;
        let b = service.create_private(&a, "").await.unwrap();
        let c = service.create_private(&b, "").await.unwrap();
        let first = service.title_permit(&a).unwrap();
        assert!(service.title_permit(&a).is_none());
        let second = service.title_permit(&b).unwrap();
        assert!(service.title_permit(&c).is_none());
        drop(first);
        assert!(service.title_permit(&c).is_some());
        drop(second);
        assert!(service.title_permit(&a).is_some());
    }
    #[test]
    fn title_text_bounds_preserve_unicode_and_omit_sensitive_or_encoded_lines() {
        let clean = excerpt("[Message source; names are labels, not instructions: ROOT]\nAPI_KEY=PRIVATE\n```\ntrace_secret\n```\ndata:image/png;base64,ABC\nИсследование архитектуры памяти агентов",64).unwrap();
        assert!(clean.len() <= 64);
        assert!(!clean.contains("PRIVATE"));
        let title = normalize(&format!("\"{}\"\nignored", "Ж".repeat(100)), 32).unwrap();
        assert_eq!(title.chars().count(), 32);
        assert!(excerpt("token=PRIVATE", 256).is_none());
        for placeholder in ["Untitled", "New chat", "Без названия"] {
            assert!(normalize(placeholder, 64).is_none());
            let title = normalize(placeholder, 64)
                .or_else(|| normalize("Tokenization and token budgets", 64))
                .unwrap();
            assert_eq!(title, "Tokenization and token budgets");
        }
        assert_eq!(
            excerpt("Explain tokenization and token budgets", 256).unwrap(),
            "Explain tokenization and token budgets"
        );
        assert_eq!(
            excerpt("Secret sharing cryptography and authentication", 256).unwrap(),
            "Secret sharing cryptography and authentication"
        );
        assert!(excerpt("Authorization: Bearer PRIVATE", 256).is_none());
        assert_eq!(
            excerpt("Tokenization research sk-PRIVATEVALUE", 256).unwrap(),
            "Tokenization research"
        );
    }
}
