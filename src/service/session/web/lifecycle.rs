use octo_history::{Session, SessionAccess, SessionId, SessionKind};

use super::{verify_actor, WebActor};
use crate::{
    db::{
        entity::MarkerState,
        error::DatabaseError,
        repo::{
            excursion::discard_in,
            marker::{find_in, set_owned},
        },
    },
    history::sessions::Backend,
    service::session::SessionService,
    sessions::error::{SessionError, SessionResult},
};

impl SessionService {
    /// Close a temporary resource across all views; persistent End only leaves
    /// the browser view. The caller publishes native work cancellation afterward.
    pub async fn web_end(
        &self,
        actor: &WebActor,
        id: &SessionId,
        expected_revision: i64,
    ) -> SessionResult<(Session, bool)> {
        let _lifecycle = self.router.lifecycle_gate().write_owned().await;
        let mut backend = self.router.handle(id).await.write_owned().await;
        let mut tx = self.history.begin_journal().await?;
        verify_actor(&mut tx, actor).await?;
        let session = self.catalog_session_in(&mut tx, id).await?;
        if session.access
            != (SessionAccess::Private {
                owner: actor.user_id.as_str().into(),
            })
        {
            return Err(SessionError::Denied);
        }
        if session.revision != expected_revision {
            return Err(DatabaseError::RevisionConflict.into());
        }
        if session.kind == SessionKind::Permanent {
            return Ok((session, false));
        }
        let marker = find_in(&mut tx, id.as_str())
            .await?
            .ok_or(SessionError::Expired)?;
        if marker.owner_user_id.as_deref() != Some(actor.user_id.as_str())
            || marker.state != MarkerState::Temporary
        {
            return Err(SessionError::Denied);
        }
        let snapshot = self.router.temporary().export_session(id)?;
        self.leases.close(id)?;
        *backend = Backend::Closed;
        set_owned(
            &mut tx,
            id.as_str(),
            &marker.runtime_epoch,
            MarkerState::Closed,
            Some(marker.revision),
            actor.user_id.as_str(),
        )
        .await?;
        discard_in(&mut tx, id.as_str()).await?;
        tx.commit().await.map_err(DatabaseError::from)?;
        self.router
            .temporary()
            .discard_session(id, snapshot.fingerprint())?;
        Ok((session, true))
    }
}
