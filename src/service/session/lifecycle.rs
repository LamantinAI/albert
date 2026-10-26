//! Retention-aware metadata access. Never await a Session backend lease while
//! holding the SQLite writer: Save takes lifecycle -> backend -> SQL in order.

use std::sync::Arc;

use octo_history::{Session, SessionId, SessionStore};
use sqlx::{Sqlite, Transaction};

use super::SessionService;
use crate::{
    db::{entity::MarkerState, error::DatabaseError, repo::marker::find_in},
    sessions::{
        error::{SessionError, SessionResult},
        lease::SessionLease,
        SessionContext,
    },
};

impl SessionService {
    pub(super) async fn catalog_session_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        id: &SessionId,
    ) -> SessionResult<Session> {
        let marker = find_in(tx, id.as_str()).await?;
        if marker
            .as_ref()
            .is_some_and(|marker| marker.owner_user_id.is_none())
        {
            return Err(SessionError::Denied);
        }
        match marker.as_ref().map(|marker| marker.state) {
            Some(MarkerState::Closed) => return Err(SessionError::Ended),
            Some(MarkerState::Expired) => return Err(SessionError::Expired),
            Some(MarkerState::Temporary) => {
                let marker = marker.as_ref().ok_or(SessionError::Unavailable)?;
                if marker.runtime_epoch != self.runtime_epoch {
                    return Err(SessionError::Expired);
                }
                return self
                    .router
                    .temporary()
                    .get_session(id)
                    .await?
                    .ok_or(SessionError::Expired);
            }
            _ => {}
        }
        self.history
            .get_session_in(tx, id)
            .await?
            .ok_or(SessionError::NotFound)
    }

    pub(super) async fn lease_for_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        id: &SessionId,
    ) -> SessionResult<Arc<SessionLease>> {
        let marker = find_in(tx, id.as_str()).await?;
        if let Some(marker) = &marker {
            if marker.owner_user_id.is_none() {
                return Err(SessionError::Denied);
            }
            match marker.state {
                MarkerState::Closed => return Err(SessionError::Ended),
                MarkerState::Expired => return Err(SessionError::Expired),
                MarkerState::Temporary if marker.runtime_epoch != self.runtime_epoch => {
                    return Err(SessionError::Expired);
                }
                _ => {}
            }
        }
        self.leases.issue(
            id,
            marker.as_ref().map(|marker| marker.runtime_epoch.as_str()),
        )
    }

    pub async fn validate_context(&self, context: &SessionContext) -> SessionResult<()> {
        if !context.is_live() {
            return Err(SessionError::Ended);
        }
        let mut tx = self.history.begin_journal().await?;
        let session = self
            .catalog_session_in(&mut tx, context.session_id())
            .await?;
        let lease = self.lease_for_in(&mut tx, context.session_id()).await?;
        if lease.epoch() != context.scope_epoch() {
            return Err(SessionError::Expired);
        }
        context.check(&session)?;
        tx.commit().await.map_err(DatabaseError::from)?;
        Ok(())
    }
}
