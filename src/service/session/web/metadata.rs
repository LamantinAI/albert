use octo_history::{CreateSession, Session, SessionId, SessionKind, SessionStore, SessionUpdate};

use super::{verify_actor, WebActor};
use crate::{
    db::{entity::MarkerState, error::DatabaseError, repo::marker::set_owned},
    history::sessions::Backend,
    service::session::SessionService,
    sessions::error::{SessionError, SessionResult},
};

impl SessionService {
    pub async fn web_create(
        &self,
        actor: &WebActor,
        title: &str,
        temporary: bool,
    ) -> SessionResult<Session> {
        if title.len() > 4096 {
            return Err(SessionError::InvalidInput("title is too long"));
        }
        let mut request = CreateSession::private(actor.user_id.as_str());
        request.title = title.into();
        if !temporary {
            let mut tx = self.history.begin_journal().await?;
            verify_actor(&mut tx, actor).await?;
            let session = self.history.create_session_in(&mut tx, request).await?;
            tx.commit().await.map_err(DatabaseError::from)?;
            return Ok(session);
        }
        let _lifecycle = self.router.lifecycle_gate().write_owned().await;
        let id = SessionId::new();
        request.id = Some(id.clone());
        request.kind = SessionKind::Temporary;
        let mut backend = self.router.handle(&id).await.write_owned().await;
        let mut tx = self.history.begin_journal().await?;
        verify_actor(&mut tx, actor).await?;
        let session = self.router.temporary().create_session(request).await?;
        let result: SessionResult<()> = async {
            set_owned(
                &mut tx,
                id.as_str(),
                &self.runtime_epoch,
                MarkerState::Temporary,
                None,
                actor.user_id.as_str(),
            )
            .await?;
            tx.commit().await.map_err(DatabaseError::from)?;
            Ok(())
        }
        .await;
        if let Err(error) = result {
            let snapshot = self.router.temporary().export_session(&id)?;
            self.router
                .temporary()
                .discard_session(&id, snapshot.fingerprint())?;
            *backend = Backend::Closed;
            return Err(error);
        }
        *backend = Backend::Temporary;
        self.leases.issue(&id, Some(&self.runtime_epoch))?;
        Ok(session)
    }

    pub async fn web_update(
        &self,
        actor: &WebActor,
        id: &SessionId,
        update: SessionUpdate,
    ) -> SessionResult<Session> {
        let _lifecycle = self.router.lifecycle_gate().read_owned().await;
        let context = self.web_context(actor, id).await?;
        if context.session().kind == SessionKind::Temporary {
            return Ok(self.router.temporary().update_session(id, update).await?);
        }
        let mut tx = self.history.begin_journal().await?;
        let session = self.history.update_session_in(&mut tx, id, update).await?;
        tx.commit().await.map_err(DatabaseError::from)?;
        Ok(session)
    }
}
