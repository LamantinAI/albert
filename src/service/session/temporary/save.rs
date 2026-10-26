use octo_connector_telegram::NativeFrame;
use tracing::warn;

use crate::{
    db::{
        entity::MarkerState,
        error::DatabaseError,
        repo::{
            excursion::promote_in,
            marker::{find, find_in, set_owned},
        },
    },
    history::sessions::Backend,
    service::session::{selected, SessionService},
    sessions::{
        error::{SessionError, SessionResult},
        SessionContext,
    },
};

impl SessionService {
    pub async fn save_temporary(
        &self,
        context: &SessionContext,
        promotion_key: &str,
        _native: Option<&NativeFrame>,
    ) -> SessionResult<SessionContext> {
        context.require_private()?;
        let global = self.router.lifecycle_gate();
        let _lifecycle = global.write_owned().await;
        let handle = self.router.handle(context.session_id()).await;
        let mut backend = handle.write_owned().await;
        let mut tx = self.history.begin_journal().await?;
        let id = context.session_id();
        let marker = find_in(&mut tx, id.as_str())
            .await?
            .ok_or(SessionError::InvalidInput("this chat is not temporary"))?;
        if marker.owner_user_id.as_deref() != Some(context.private_user_id()?.as_str()) {
            return Err(SessionError::Denied);
        }
        if matches!(marker.state, MarkerState::Closed | MarkerState::Expired) {
            return Err(SessionError::Expired);
        }
        if marker.state == MarkerState::Saved {
            let session = self.catalog_session_in(&mut tx, id).await?;
            let lease = self.lease_for_in(&mut tx, id).await?;
            context.check(&session)?;
            tx.commit().await.map_err(DatabaseError::from)?;
            *backend = Backend::Durable;
            return Ok(selected(
                context,
                session,
                context.selector_revision().unwrap_or(1),
                lease,
                context.cleanup_scope().map(str::to_owned),
            ));
        }
        if marker.runtime_epoch != self.runtime_epoch {
            return Err(SessionError::Expired);
        }
        let snapshot = self.router.temporary().export_session(id)?;
        context.check(snapshot.session())?;
        self.history
            .promote_session_in(&mut tx, &snapshot, promotion_key)
            .await?;
        set_owned(
            &mut tx,
            id.as_str(),
            &marker.runtime_epoch,
            MarkerState::Saved,
            Some(marker.revision),
            context.private_user_id()?.as_str(),
        )
        .await?;
        promote_in(&mut tx, id.as_str()).await?;
        let session = self.catalog_session_in(&mut tx, id).await?;
        let lease = self.lease_for_in(&mut tx, id).await?;
        if let Err(error) = tx.commit().await {
            // Reconcile an uncertain acknowledgement before any writer can
            // choose a backend. Marker + promotion share the same transaction.
            match find(self.database.pool(), id.as_str()).await {
                Ok(Some(current))
                    if current.runtime_epoch == marker.runtime_epoch
                        && current.state == MarkerState::Saved
                        && current.owner_user_id == marker.owner_user_id => {}
                Ok(Some(current)) if current == marker => {
                    return Err(DatabaseError::from(error).into())
                }
                _ => {
                    self.leases.close(id)?;
                    *backend = Backend::Closed;
                    return Err(SessionError::Unavailable);
                }
            }
        }
        *backend = Backend::Durable;
        // If cleanup fails, durable Save remains committed and all writers use
        // SQL. Keeping a redundant RAM copy cannot undo the successful Save.
        if self
            .router
            .temporary()
            .discard_session(id, snapshot.fingerprint())
            .is_err()
        {
            warn!("saved session retained a redundant RAM snapshot; all writers use SQLite");
        }
        Ok(selected(
            context,
            session,
            context.selector_revision().unwrap_or(1),
            lease,
            context.cleanup_scope().map(str::to_owned),
        ))
    }
}
