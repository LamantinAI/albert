use octo_history::{
    AppendRequest, CreateSession, HistoryStore, Role, SessionId, SessionJournal, SessionKind,
    SessionStore,
};
use sqlx::{query, query_as, query_scalar, Sqlite, Transaction};

use super::{verify_actor, WebActor};
use crate::{
    db::entity::MarkerState, db::error::DatabaseError, db::repo::marker::set_owned,
    history::assistant_turn, history::preview_turn, history::sessions::Backend,
    history::PreviewContent, media::format_assets, service::session::SessionService,
    sessions::actor::ActorToolPolicy, sessions::error::SessionError,
    sessions::error::SessionResult,
};

type Row = (String, String, String, bool, String, String, String, bool);
pub struct StartRecord {
    pub request: String,
    pub session: String,
    pub phase: String,
    pub temporary: bool,
    pub epoch: String,
    pub digest: String,
    pub account: String,
    pub host_tools: bool,
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use octo_history::{AppendRequest, HistoryStore, SessionId, SqliteHistory, Turn};
    use tempfile::tempdir;

    use crate::{
        db::repo::identity::bind, service::session::web::WebActor,
        service::session::SessionService, sessions::actor::ActorToolPolicy,
        sessions::settings::SessionSettings, sessions::UserId,
    };

    #[tokio::test]
    async fn admission_is_idempotent_and_publication_requires_terminal_evidence() {
        let dir = tempdir().unwrap();
        let history = Arc::new(
            SqliteHistory::open_retained(dir.path().join("history.db"))
                .await
                .unwrap(),
        );
        let service = SessionService::from_history(history.clone(), SessionSettings::default())
            .await
            .unwrap();
        let mut tx = history.begin_journal().await.unwrap();
        bind(&mut tx, "web", "account", "person").await.unwrap();
        tx.commit().await.unwrap();
        let actor = WebActor {
            account_id: "account".into(),
            user_id: UserId::parse("person").unwrap(),
            tools: ActorToolPolicy::ExistingHost,
        };
        for temporary in [false, true] {
            let request = SessionId::new().to_string();
            let input = AppendRequest {
                namespace: "fixture-start".into(),
                idempotency_key: request.clone(),
                digest: "body".into(),
                turns: vec![Turn::user("First input")],
            };
            let row = service
                .web_start_create(&actor, &request, "request-body", temporary, input.clone())
                .await
                .unwrap();
            let duplicate = service
                .web_start_create(&actor, &request, "request-body", temporary, input.clone())
                .await
                .unwrap();
            assert_eq!(row.session, duplicate.session);
            assert!(service
                .web_start_create(&actor, &request, "conflict", temporary, input)
                .await
                .is_err());
            let id = SessionId::parse(&row.session).unwrap();
            assert!(service.web_unpublished(&id).await.unwrap());
            assert!(service.web_claim_start(&actor, &request).await.unwrap());
            assert!(!service.web_claim_start(&actor, &request).await.unwrap());
            assert!(service.web_run_start(&id, &request).await.unwrap());
            assert!(!service.web_run_start(&id, &request).await.unwrap());
            let context = service.web_context(&actor, &id).await.unwrap();
            assert_eq!(context.chat_only(), false);
            service
                .journal_router()
                .append_journal(
                    &context.root_history_key(),
                    &AppendRequest {
                        namespace: "albert/host".into(),
                        idempotency_key: "scope/final".into(),
                        digest: "answer".into(),
                        turns: vec![Turn::assistant("First answer")],
                    },
                )
                .await
                .unwrap();
            service
                .web_fail_start(&actor, &row, "Should not overwrite committed answer")
                .await
                .unwrap();
            assert!(!service.web_unpublished(&id).await.unwrap());
            let turns = service
                .journal_router()
                .load(&context.root_history_key())
                .await;
            assert_eq!(turns.len(), 2);
        }
        let request = SessionId::new().to_string();
        let row = service
            .web_start_create(
                &actor,
                &request,
                "pending",
                false,
                AppendRequest {
                    namespace: "fixture".into(),
                    idempotency_key: request.clone(),
                    digest: "pending".into(),
                    turns: vec![Turn::user("Pending request")],
                },
            )
            .await
            .unwrap();
        drop(service);
        let restarted = SessionService::from_history(history, SessionSettings::default())
            .await
            .unwrap();
        let recovered = restarted
            .web_start_record(&actor, &request)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(recovered.phase, "failed");
        assert!(!restarted
            .web_unpublished(&SessionId::parse(&row.session).unwrap())
            .await
            .unwrap());
        assert!(!restarted.web_claim_start(&actor, &request).await.unwrap());
    }
}
impl StartRecord {
    fn row(row: Row) -> Self {
        Self {
            request: row.0,
            session: row.1,
            phase: row.2,
            temporary: row.3,
            epoch: row.4,
            digest: row.5,
            account: row.6,
            host_tools: row.7,
        }
    }
    pub fn published(&self) -> bool {
        matches!(self.phase.as_str(), "complete" | "failed")
    }
}

impl SessionService {
    pub async fn web_start_record(
        &self,
        actor: &WebActor,
        request: &str,
    ) -> SessionResult<Option<StartRecord>> {
        let mut tx = self.history.begin_journal().await?;
        verify_actor(&mut tx, actor).await?;
        let row=query_as::<_,Row>("SELECT request_id,session_id,phase,temporary,runtime_epoch,digest,account_id,host_tools FROM albert_web_starts WHERE owner_user_id=? AND request_id=?").bind(actor.user_id.as_str()).bind(request).fetch_optional(&mut *tx).await.map_err(DatabaseError::from)?;
        tx.commit().await.map_err(DatabaseError::from)?;
        Ok(row.map(StartRecord::row))
    }
    pub async fn web_start_create(
        &self,
        actor: &WebActor,
        request_id: &str,
        digest: &str,
        temporary: bool,
        input: AppendRequest,
    ) -> SessionResult<StartRecord> {
        let _life = self.router.lifecycle_gate().write_owned().await;
        let id = SessionId::new();
        let handle = self.router.handle(&id).await;
        let mut backend = handle.write_owned().await;
        let mut tx = self.history.begin_journal().await?;
        verify_actor(&mut tx, actor).await?;
        if let Some(row)=query_as::<_,Row>("SELECT request_id,session_id,phase,temporary,runtime_epoch,digest,account_id,host_tools FROM albert_web_starts WHERE owner_user_id=? AND request_id=?").bind(actor.user_id.as_str()).bind(request_id).fetch_optional(&mut *tx).await.map_err(DatabaseError::from)? {
            if row.5!=digest {return Err(SessionError::InvalidInput("start request conflicts with accepted input"));}
            tx.commit().await.map_err(DatabaseError::from)?;return Ok(StartRecord::row(row));
        }
        let pending:i64=query_scalar("SELECT COUNT(*) FROM albert_web_starts WHERE owner_user_id=? AND phase IN ('accepted','claimed','running')").bind(actor.user_id.as_str()).fetch_one(&mut *tx).await.map_err(DatabaseError::from)?;
        let total:i64=query_scalar("SELECT COUNT(*) FROM albert_web_starts WHERE phase IN ('accepted','claimed','running')").fetch_one(&mut *tx).await.map_err(DatabaseError::from)?;
        if pending >= 8 || total >= 64 {
            return Err(SessionError::InvalidInput("too many pending first runs"));
        }
        let mut create = CreateSession::private(actor.user_id.as_str());
        create.id = Some(id.clone());
        if temporary {
            create.kind = SessionKind::Temporary;
        }
        let journal = SessionJournal::Root(id.clone());
        let media_assets = format_assets(&input.turns);
        let reserved = if temporary {
            self.media()
                .bind_temporary(id.as_str(), &media_assets)
                .await
        } else {
            self.media()
                .reserve_durable(id.as_str(), &media_assets)
                .await
        }
        .map_err(|_| SessionError::InvalidInput("Attachments are unavailable."))?;
        let accepted:SessionResult<()> = async {
            if temporary {self.router.temporary().create_session(create).await?;}else{self.history.create_session_in(&mut tx,create).await?;}
            if temporary {
                self.router.temporary().append_journal(&journal.storage_key()?,&input).await?;
                set_owned(&mut tx,id.as_str(),&self.runtime_epoch,MarkerState::Temporary,None,actor.user_id.as_str()).await?;
            } else {
                self.media().persist_in(&mut tx,id.as_str(),&media_assets).await.map_err(|_| SessionError::InvalidInput("Attachments could not be stored."))?;
                self.history.append_session_in(&mut tx,&journal,&input).await?;
            }
            query("INSERT INTO albert_web_starts(owner_user_id,request_id,session_id,digest,temporary,phase,runtime_epoch,account_id,host_tools) VALUES (?,?,?,?,?,'accepted',?,?,?)").bind(actor.user_id.as_str()).bind(request_id).bind(id.as_str()).bind(digest).bind(temporary).bind(&self.runtime_epoch).bind(&actor.account_id).bind(actor.tools==ActorToolPolicy::ExistingHost).execute(&mut *tx).await.map_err(DatabaseError::from)?;
            tx.commit().await.map_err(DatabaseError::from)?;Ok(())
        }.await;
        if let Err(error) = accepted {
            // A lost commit acknowledgement must not release assets already
            // accepted in SQL. Reconcile before dropping any reservation.
            match self.web_start_record(actor, request_id).await {
                Ok(Some(row)) if row.session == id.as_str() && row.digest == digest => {}
                Ok(None) => {
                    if temporary {
                        if let Ok(snapshot) = self.router.temporary().export_session(&id) {
                            self.router
                                .temporary()
                                .discard_session(&id, snapshot.fingerprint())?;
                        }
                    }
                    self.media()
                        .rollback_temporary(id.as_str(), &reserved)
                        .await;
                    *backend = Backend::Closed;
                    return Err(error);
                }
                _ => {
                    *backend = Backend::Closed;
                    return Err(SessionError::Unavailable);
                }
            }
        }
        *backend = if temporary {
            Backend::Temporary
        } else {
            Backend::Durable
        };
        if temporary {
            self.leases.issue(&id, Some(&self.runtime_epoch))?;
        } else {
            self.media().release_durable(&media_assets).await;
        }
        Ok(StartRecord {
            request: request_id.into(),
            session: id.to_string(),
            phase: "accepted".into(),
            temporary,
            epoch: self.runtime_epoch.clone(),
            digest: digest.into(),
            account: actor.account_id.clone(),
            host_tools: actor.tools == ActorToolPolicy::ExistingHost,
        })
    }
    pub async fn web_claim_start(&self, actor: &WebActor, request: &str) -> SessionResult<bool> {
        let mut tx = self.history.begin_journal().await?;
        verify_actor(&mut tx, actor).await?;
        let changed=query("UPDATE albert_web_starts SET phase='claimed' WHERE owner_user_id=? AND request_id=? AND phase='accepted' AND runtime_epoch=? AND account_id=? AND host_tools=?").bind(actor.user_id.as_str()).bind(request).bind(&self.runtime_epoch).bind(&actor.account_id).bind(actor.tools==ActorToolPolicy::ExistingHost).execute(&mut *tx).await.map_err(DatabaseError::from)?.rows_affected();
        tx.commit().await.map_err(DatabaseError::from)?;
        Ok(changed == 1)
    }
    pub(crate) async fn web_run_start(&self, id: &SessionId, request: &str) -> SessionResult<bool> {
        let changed=query("UPDATE albert_web_starts SET phase='running' WHERE session_id=? AND request_id=? AND phase='claimed' AND runtime_epoch=?").bind(id.as_str()).bind(request).bind(&self.runtime_epoch).execute(self.database.pool()).await.map_err(DatabaseError::from)?.rows_affected();
        Ok(changed == 1)
    }
    pub async fn web_unpublished(&self, id: &SessionId) -> SessionResult<bool> {
        Ok(query_scalar::<_,i64>("SELECT COUNT(*) FROM albert_web_starts WHERE session_id=? AND phase NOT IN ('complete','failed')").bind(id.as_str()).fetch_one(self.database.pool()).await.map_err(DatabaseError::from)?>0)
    }
    pub async fn web_waiting_for_admission(&self, id: &SessionId) -> SessionResult<bool> {
        Ok(query_scalar::<_,i64>("SELECT COUNT(*) FROM albert_web_starts WHERE session_id=? AND phase IN ('accepted','claimed')").bind(id.as_str()).fetch_one(self.database.pool()).await.map_err(DatabaseError::from)?>0)
    }
    pub(crate) async fn web_finish_start(&self, id: &SessionId) -> SessionResult<()> {
        query("UPDATE albert_web_starts SET phase='complete' WHERE session_id=? AND phase IN ('accepted','claimed','running')").bind(id.as_str()).execute(self.database.pool()).await.map_err(DatabaseError::from)?;
        Ok(())
    }
    pub async fn web_fail_start(
        &self,
        actor: &WebActor,
        record: &StartRecord,
        text: &str,
    ) -> SessionResult<()> {
        if record.published() || record.phase == "expired" {
            return Ok(());
        }
        let id = SessionId::parse(&record.session)?;
        let saved: bool = query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM albert_session_markers WHERE session_id=? AND state='saved'",
        )
        .bind(id.as_str())
        .fetch_one(self.database.pool())
        .await
        .map_err(DatabaseError::from)?
            > 0;
        if record.temporary && !saved && record.epoch != self.runtime_epoch {
            query("UPDATE albert_web_starts SET phase='expired' WHERE session_id=?")
                .bind(id.as_str())
                .execute(self.database.pool())
                .await
                .map_err(DatabaseError::from)?;
            return Ok(());
        }
        if !record.temporary || saved {
            let mut tx = self.history.begin_journal().await?;
            verify_actor(&mut tx, actor).await?;
            self.fail_start_in(&mut tx, record, text).await?;
            tx.commit().await.map_err(DatabaseError::from)?;
            return Ok(());
        }
        let context = self.web_context(actor, &id).await?;
        let turns = self.router.load(&context.root_history_key()).await;
        if turns.iter().any(|turn| {
            matches!(preview_turn(turn), PreviewContent::Text(_)) && turn.role == Role::Assistant
        }) {
            return self.web_finish_start(&id).await;
        }
        self.router
            .append_journal(
                &context.root_history_key(),
                &AppendRequest {
                    namespace: "albert/web-start-terminal".into(),
                    idempotency_key: record.request.clone(),
                    digest: "interrupted-v1".into(),
                    turns: vec![assistant_turn(text.into())],
                },
            )
            .await?;
        query("UPDATE albert_web_starts SET phase='failed' WHERE session_id=? AND phase IN ('accepted','claimed','running')").bind(id.as_str()).execute(self.database.pool()).await.map_err(DatabaseError::from)?;
        Ok(())
    }
    async fn fail_start_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        record: &StartRecord,
        text: &str,
    ) -> SessionResult<()> {
        let phase: Option<String> =
            query_scalar("SELECT phase FROM albert_web_starts WHERE session_id=?")
                .bind(&record.session)
                .fetch_optional(&mut **tx)
                .await
                .map_err(DatabaseError::from)?;
        if !phase.is_some_and(|phase| matches!(phase.as_str(), "accepted" | "claimed" | "running"))
        {
            return Ok(());
        }
        let journal = SessionJournal::Root(SessionId::parse(&record.session)?);
        let key = journal.storage_key()?;
        let committed:i64=query_scalar("SELECT COUNT(*) FROM journal_appends WHERE channel=? AND namespace='albert/host' AND (idempotency_key LIKE '%/final' OR idempotency_key LIKE '%/reflex-result')").bind(&key).fetch_one(&mut **tx).await.map_err(DatabaseError::from)?;
        if committed == 0 {
            self.history
                .append_session_in(
                    tx,
                    &journal,
                    &AppendRequest {
                        namespace: "albert/web-start-terminal".into(),
                        idempotency_key: record.request.clone(),
                        digest: "interrupted-v1".into(),
                        turns: vec![assistant_turn(text.into())],
                    },
                )
                .await?;
        }
        query("UPDATE albert_web_starts SET phase=? WHERE session_id=?")
            .bind(if committed > 0 { "complete" } else { "failed" })
            .bind(&record.session)
            .execute(&mut **tx)
            .await
            .map_err(DatabaseError::from)?;
        Ok(())
    }
    pub(crate) async fn recover_web_starts(&self) -> SessionResult<()> {
        let rows=query_as::<_,Row>("SELECT request_id,session_id,phase,temporary,runtime_epoch,digest,account_id,host_tools FROM albert_web_starts WHERE phase IN ('accepted','claimed','running') AND runtime_epoch!=? LIMIT 64").bind(&self.runtime_epoch).fetch_all(self.database.pool()).await.map_err(DatabaseError::from)?;
        for row in rows {
            let row = StartRecord::row(row);
            let saved: bool = query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM albert_session_markers WHERE session_id=? AND state='saved'",
            )
            .bind(&row.session)
            .fetch_one(self.database.pool())
            .await
            .map_err(DatabaseError::from)?
                > 0;
            if row.temporary && !saved {
                query("UPDATE albert_web_starts SET phase='expired' WHERE session_id=?")
                    .bind(&row.session)
                    .execute(self.database.pool())
                    .await
                    .map_err(DatabaseError::from)?;
            } else {
                let mut tx = self.history.begin_journal().await?;
                self.fail_start_in(&mut tx,&row,"The first run was interrupted by a restart. Check recorded actions before starting it again.").await?;
                tx.commit().await.map_err(DatabaseError::from)?;
            }
        }
        Ok(())
    }
    pub async fn web_reconcile_start(
        &self,
        actor: &WebActor,
        record: &StartRecord,
    ) -> SessionResult<()> {
        if record.temporary {
            let marker: Option<String> =
                query_scalar("SELECT state FROM albert_session_markers WHERE session_id=?")
                    .bind(&record.session)
                    .fetch_optional(self.database.pool())
                    .await
                    .map_err(DatabaseError::from)?;
            if marker
                .as_deref()
                .is_some_and(|value| matches!(value, "closed" | "expired"))
                || (record.epoch != self.runtime_epoch && marker.as_deref() != Some("saved"))
            {
                query("UPDATE albert_web_starts SET phase='expired' WHERE session_id=?")
                    .bind(&record.session)
                    .execute(self.database.pool())
                    .await
                    .map_err(DatabaseError::from)?;
                return Ok(());
            }
        }
        if record.published() || record.phase == "expired" {
            return Ok(());
        }
        if record.epoch != self.runtime_epoch {
            return self.web_fail_start(actor,record,"The first run was interrupted by a restart. Check recorded actions before starting it again.").await;
        }
        if !record.temporary {
            let key = SessionJournal::Root(SessionId::parse(&record.session)?).storage_key()?;
            let committed:i64=query_scalar("SELECT COUNT(*) FROM journal_appends WHERE channel=? AND namespace='albert/host' AND (idempotency_key LIKE '%/final' OR idempotency_key LIKE '%/reflex-result')").bind(key).fetch_one(self.database.pool()).await.map_err(DatabaseError::from)?;
            if committed > 0 {
                self.web_finish_start(&SessionId::parse(&record.session)?)
                    .await?;
            }
        }
        Ok(())
    }
}
