use octo_connector_telegram::NativeFrame;
use octo_history::{CreateSession, SessionId, SessionKind, SessionStore};

use crate::{
    db::{
        entity::MarkerState,
        error::DatabaseError,
        repo::{
            marker::{find_in as find_marker, set_owned},
            route::{find_in as find_route, set},
        },
    },
    history::sessions::Backend,
    service::session::{route_key, selected, SessionService},
    sessions::{
        cleanup_scope_for,
        error::{SessionError, SessionResult},
        SessionContext,
    },
};

impl SessionService {
    pub async fn start_temporary(
        &self,
        context: &SessionContext,
        native: Option<&NativeFrame>,
    ) -> SessionResult<SessionContext> {
        context.require_private()?;
        let global = self.router.lifecycle_gate();
        let _lifecycle = global.write_owned().await;
        let id = SessionId::new();
        let handle = self.router.handle(&id).await;
        let mut backend = handle.write_owned().await;
        let mut tx = self.history.begin_journal().await?;
        let key = route_key(context)?;
        let route = find_route(&mut tx, &key)
            .await?
            .ok_or(SessionError::NotFound)?;
        let previous_id = SessionId::parse(&route.session_id)?;
        let marker = find_marker(&mut tx, previous_id.as_str()).await?;
        if let Some(marker) = &marker {
            if marker.owner_user_id.as_deref() != Some(context.private_user_id()?.as_str()) {
                return Err(SessionError::Denied);
            }
            if marker.state == MarkerState::Temporary && marker.runtime_epoch == self.runtime_epoch
            {
                let session = self.catalog_session_in(&mut tx, &previous_id).await?;
                let lease = self.lease_for_in(&mut tx, &previous_id).await?;
                if !lease.is_live() {
                    return Err(SessionError::Ended);
                }
                tx.commit().await.map_err(DatabaseError::from)?;
                let cleanup = Some(cleanup_scope_for(context.reply_route(), &previous_id));
                return Ok(selected(context, session, route.revision, lease, cleanup));
            }
            if marker.state == MarkerState::Saved && route.return_session_id.is_some() {
                return Err(SessionError::InvalidInput(
                    "use /end before starting another temporary chat",
                ));
            }
        }
        if context
            .selector_revision()
            .is_some_and(|revision| revision != route.revision)
        {
            return Err(DatabaseError::RevisionConflict.into());
        }
        let return_id = match marker.as_ref().map(|marker| marker.state) {
            Some(MarkerState::Closed | MarkerState::Expired) => route
                .return_session_id
                .as_deref()
                .ok_or(SessionError::Unavailable)?
                .to_owned(),
            _ => route.session_id.clone(),
        };
        let return_session = self
            .catalog_session_in(&mut tx, &SessionId::parse(&return_id)?)
            .await?;
        context.check(&return_session)?;
        let mut request = CreateSession::private(context.private_user_id()?.as_str());
        request.id = Some(id.clone());
        request.kind = SessionKind::Temporary;
        let session = self.router.temporary().create_session(request).await?;
        let writes: SessionResult<i64> = async {
            self.start_native_in(&mut tx, context, &id, &return_id, native)
                .await?;
            set_owned(
                &mut tx,
                id.as_str(),
                &self.runtime_epoch,
                MarkerState::Temporary,
                None,
                context.private_user_id()?.as_str(),
            )
            .await?;
            Ok(set(
                &mut tx,
                &key,
                id.as_str(),
                Some(&return_id),
                Some(route.revision),
            )
            .await?)
        }
        .await;
        let revision = match writes {
            Ok(revision) => revision,
            Err(error) => {
                let rollback = tx.rollback().await;
                let snapshot = self.router.temporary().export_session(&id)?;
                self.router
                    .temporary()
                    .discard_session(&id, snapshot.fingerprint())?;
                *backend = Backend::Closed;
                rollback.map_err(DatabaseError::from)?;
                return Err(error);
            }
        };
        tx.commit().await.map_err(DatabaseError::from)?;
        *backend = Backend::Temporary;
        self.presentations
            .record(&key, &previous_id, route.revision, &id, revision);
        let lease = self.leases.issue(&id, Some(&self.runtime_epoch))?;
        Ok(selected(
            context,
            session,
            revision,
            lease,
            Some(cleanup_scope_for(context.reply_route(), &id)),
        ))
    }
}
