use octo_connector_telegram::NativeFrame;
use octo_history::{CreateSession, SessionId, SessionKind};

use super::{route_key, selected, SessionService};
use crate::{
    db::{
        entity::MarkerState,
        error::DatabaseError,
        repo::{
            marker::find_in as marker_in,
            route::{find_in, set},
        },
    },
    sessions::{
        cleanup_scope_for,
        error::{SessionError, SessionResult},
        SessionContext,
    },
};

impl SessionService {
    pub async fn create_private(
        &self,
        context: &SessionContext,
        title: &str,
    ) -> SessionResult<SessionContext> {
        self.create_private_native(context, title, None).await
    }

    pub async fn create_private_native(
        &self,
        context: &SessionContext,
        title: &str,
        native: Option<&NativeFrame>,
    ) -> SessionResult<SessionContext> {
        self.navigate(
            context,
            None,
            Some(title),
            context.selector_revision(),
            native,
        )
        .await
    }

    pub async fn open_private(
        &self,
        context: &SessionContext,
        id: &SessionId,
        expected_revision: i64,
    ) -> SessionResult<SessionContext> {
        self.open_private_native(context, id, expected_revision, None)
            .await
    }

    pub async fn open_private_native(
        &self,
        context: &SessionContext,
        id: &SessionId,
        expected_revision: i64,
        native: Option<&NativeFrame>,
    ) -> SessionResult<SessionContext> {
        self.navigate(context, Some(id), None, Some(expected_revision), native)
            .await
    }

    async fn navigate(
        &self,
        context: &SessionContext,
        target: Option<&SessionId>,
        title: Option<&str>,
        expected_revision: Option<i64>,
        native: Option<&NativeFrame>,
    ) -> SessionResult<SessionContext> {
        context.require_private()?;
        let global = self.router.lifecycle_gate();
        let _lifecycle = global.write_owned().await;
        let mut tx = self.history.begin_journal().await?;
        let key = route_key(context)?;
        let route = find_in(&mut tx, &key)
            .await?
            .ok_or(SessionError::NotFound)?;
        if Some(route.revision) != expected_revision {
            return Err(DatabaseError::RevisionConflict.into());
        }
        let old_id = SessionId::parse(&route.session_id)?;
        if route.return_session_id.is_some() {
            let marker = marker_in(&mut tx, old_id.as_str())
                .await?
                .ok_or(SessionError::Unavailable)?;
            if marker.owner_user_id.as_deref() != Some(context.private_user_id()?.as_str()) {
                return Err(SessionError::Denied);
            }
            if !matches!(marker.state, MarkerState::Closed | MarkerState::Expired) {
                return Err(SessionError::InvalidInput(
                    "use /end before selecting another chat",
                ));
            }
            self.close_native_in(&mut tx, context, &old_id, native)
                .await?;
        }
        let session = match target {
            Some(id) => self.catalog_session_in(&mut tx, id).await?,
            None => {
                let mut request = CreateSession::private(context.private_user_id()?.as_str());
                request.title = title.unwrap_or_default().into();
                self.history.create_session_in(&mut tx, request).await?
            }
        };
        context.check(&session)?;
        let return_id = if session.kind == SessionKind::Temporary {
            let original = route
                .return_session_id
                .as_deref()
                .unwrap_or(&route.session_id);
            self.start_native_in(&mut tx, context, &session.id, original, native)
                .await?;
            Some(original)
        } else {
            None
        };
        let revision = set(
            &mut tx,
            &key,
            session.id.as_str(),
            return_id,
            Some(route.revision),
        )
        .await?;
        let lease = self.lease_for_in(&mut tx, &session.id).await?;
        let cleanup = return_id.map(|_| cleanup_scope_for(context.reply_route(), &session.id));
        tx.commit().await.map_err(DatabaseError::from)?;
        Ok(selected(context, session, revision, lease, cleanup))
    }
}
