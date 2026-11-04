use octo_history::{HistoryError, SessionId};

use super::EndResult;
use crate::{
    db::entity::MarkerState, db::entity::PrivateRoute, db::entity::PrivateRouteKey,
    db::error::DatabaseError, db::repo::excursion::discard_in,
    db::repo::marker::find_in as find_marker, db::repo::marker::set_owned, db::repo::route::find,
    db::repo::route::find_in, db::repo::route::set, history::sessions::Backend,
    service::session::route_key, service::session::selected, service::session::SessionService,
    sessions::cleanup_scope_for, sessions::error::SessionError, sessions::error::SessionResult,
    sessions::transport::NativeNavigation, sessions::SessionContext,
};

impl SessionService {
    pub async fn end_temporary(
        &self,
        context: &SessionContext,
        native: Option<&NativeNavigation>,
    ) -> SessionResult<EndResult> {
        context.require_private()?;
        let global = self.router.lifecycle_gate();
        let _lifecycle = global.write_owned().await;
        let key = route_key(context)?;
        let observed = find_in_pool(self, &key).await?;
        let id = SessionId::parse(&observed.session_id)?;
        let handle = self.router.handle(&id).await;
        let mut backend = handle.write_owned().await;
        let mut tx = self.history.begin_journal().await?;
        let route = find_in(&mut tx, &key)
            .await?
            .ok_or(SessionError::NotFound)?;
        if route != observed {
            return Err(DatabaseError::RevisionConflict.into());
        }
        let return_id = SessionId::parse(route.return_session_id.as_deref().ok_or(
            SessionError::InvalidInput("no temporary chat excursion is open"),
        )?)?;
        let marker = find_marker(&mut tx, id.as_str())
            .await?
            .ok_or(SessionError::Unavailable)?;
        if marker.owner_user_id.as_deref() != Some(context.private_user_id()?.as_str()) {
            return Err(SessionError::Denied);
        }
        let original = self.catalog_session_in(&mut tx, &return_id).await?;
        // Expired control admission is built from this original private Session.
        context.check(&original)?;
        self.close_native_in(&mut tx, context, &id, native).await?;
        let discarded = marker.state != MarkerState::Saved;
        let snapshot = if discarded {
            match self.router.temporary().export_session(&id) {
                Ok(snapshot) => Some(snapshot),
                Err(HistoryError::SessionNotFound)
                    if matches!(marker.state, MarkerState::Expired | MarkerState::Closed) =>
                {
                    None
                }
                Err(error) => return Err(error.into()),
            }
        } else {
            None
        };
        if discarded {
            // Close shared liveness before persistence; a failed close never
            // silently reopens tools/output. RAM is retained until SQL commits.
            self.leases.close(&id)?;
            *backend = Backend::Closed;
            set_owned(
                &mut tx,
                id.as_str(),
                &marker.runtime_epoch,
                MarkerState::Closed,
                Some(marker.revision),
                context.private_user_id()?.as_str(),
            )
            .await?;
            discard_in(&mut tx, id.as_str()).await?;
        }
        let revision = set(
            &mut tx,
            &key,
            return_id.as_str(),
            None,
            Some(route.revision),
        )
        .await?;
        let lease = self.lease_for_in(&mut tx, &return_id).await?;
        tx.commit().await.map_err(DatabaseError::from)?;
        self.presentations
            .record(&key, &id, route.revision, &return_id, revision);
        let ram_discarded = match snapshot {
            Some(snapshot) => self
                .router
                .temporary()
                .discard_session(&id, snapshot.fingerprint())
                .is_ok(),
            None => discarded,
        };
        Ok(EndResult {
            cleanup_scope: cleanup_scope_for(context.reply_route(), &id),
            scope_epoch: marker.runtime_epoch,
            session_id: id,
            discarded,
            ram_discarded,
            context: selected(context, original, revision, lease, None),
        })
    }
}

async fn find_in_pool(
    service: &SessionService,
    key: &PrivateRouteKey,
) -> SessionResult<PrivateRoute> {
    find(service.database().pool(), key)
        .await?
        .ok_or(SessionError::NotFound)
}
