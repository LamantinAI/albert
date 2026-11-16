use std::path::Path;

use super::{Row, StartRecord};
use crate::service::session::web::{verify_actor, WebActor};
use crate::{
    db::entity::MarkerState, db::error::DatabaseError, db::repo::marker::set_owned,
    history::sessions::Backend, media::decode as decode_media, media::format_assets,
    service::session::SessionService, sessions::actor::ActorToolPolicy,
    sessions::error::SessionError, sessions::error::SessionResult,
};
use octo_history::{
    AppendRequest, CreateSession, HistoryStore, SessionId, SessionJournal, SessionKind,
    SessionStore,
};
use sqlx::{query, query_as, query_scalar};
impl SessionService {
    pub async fn web_start_create(
        &self,
        actor: &WebActor,
        request_id: &str,
        digest: &str,
        temporary: bool,
        input: AppendRequest,
    ) -> SessionResult<StartRecord> {
        self.web_start_create_prepared(actor, request_id, digest, temporary, input, None)
            .await
    }
    pub async fn web_start_create_prepared(
        &self,
        actor: &WebActor,
        request_id: &str,
        digest: &str,
        temporary: bool,
        input: AppendRequest,
        workspace: Option<&Path>,
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
                if let Some(root) = workspace {
                    let copies = input.turns.iter().filter_map(|turn| decode_media(&turn.content))
                        .flat_map(|record| record.workspace_files).map(|file| file.asset_id)
                        .filter(|id| media_assets.iter().any(|asset| &asset.id == id)).collect::<Vec<_>>();
                    self.media().register_workspace_copies_in(&mut tx, id.as_str(), &copies, root).await
                        .map_err(|_| SessionError::InvalidInput("Workspace preparation ownership could not be stored."))?;
                }
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
}
