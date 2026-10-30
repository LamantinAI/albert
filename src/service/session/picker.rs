//! Read a verified existing private selector for a callback. Never create or
//! repair a route/default from an old control surface.
use octo_history::SessionId;

use super::SessionService;
use crate::{
    db::{
        entity::PrivateRouteKey,
        error::DatabaseError,
        repo::{identity::find_in as find_identity, route::find_in},
    },
    sessions::{
        cleanup_scope_for,
        error::{SessionError, SessionResult},
        AdmissionScope, ExternalIdentity, ReplyRoute, SessionContext,
    },
};
impl SessionService {
    pub(crate) async fn picker_context(
        &self,
        identity: ExternalIdentity,
        reply: ReplyRoute,
        expected_user: &str,
    ) -> SessionResult<SessionContext> {
        let _lifecycle = self.router.lifecycle_gate().read_owned().await;
        let mut tx = self.history.begin_journal().await?;
        if find_identity(&mut tx, identity.connector(), identity.actor())
            .await?
            .is_none()
        {
            return Err(SessionError::Denied);
        }
        let user = self.resolve_identity_in(&mut tx, &identity).await?;
        if user.as_str() != expected_user {
            return Err(SessionError::Denied);
        }
        let key = PrivateRouteKey {
            connector_id: reply.connector().into(),
            channel_id: reply.channel().into(),
            user_id: user.as_str().into(),
        };
        let route = find_in(&mut tx, &key)
            .await?
            .ok_or(SessionError::NotFound)?;
        let id = SessionId::parse(&route.session_id)?;
        let session = self.catalog_session_in(&mut tx, &id).await?;
        let default = self
            .history
            .get_default_session_in(&mut tx, &reply.origin())
            .await?
            .ok_or(SessionError::NotFound)?;
        let context = SessionContext {
            actor_tools: None,
            lease: self.lease_for_in(&mut tx, &id).await?,
            cleanup_scope: route
                .return_session_id
                .as_ref()
                .map(|_| cleanup_scope_for(&reply, &id)),
            user: Some(user),
            identity: Some(identity),
            reply,
            scope: AdmissionScope::Private,
            session,
            default_id: default.id.clone(),
            selector_revision: Some(route.revision),
        };
        context.check(&default)?;
        context.check(context.session())?;
        tx.commit().await.map_err(DatabaseError::from)?;
        self.presentations.observe(&key, &id, route.revision);
        Ok(context)
    }
}
