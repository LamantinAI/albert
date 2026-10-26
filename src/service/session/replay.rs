//! Recover the original Session of a native ingress receipt without changing
//! the channel's selected chat or trusting an old actor's tool permissions.

use octo_history::{SessionId, SessionKind};

use super::{route_key, SessionService};
use crate::{
    db::{error::DatabaseError, repo::route::find_in},
    sessions::{
        cleanup_scope_for,
        error::{SessionError, SessionResult},
        require_host, Admission, AdmissionScope, SessionContext,
    },
};

impl SessionService {
    pub async fn context_for_message(
        &self,
        admission: Admission,
        bound_id: &SessionId,
    ) -> SessionResult<SessionContext> {
        require_host(&admission.host)?;
        let user = match &admission.identity {
            Some(identity) => Some(self.resolve_identity(identity).await?),
            None if admission.scope == AdmissionScope::Group => None,
            None => return Err(SessionError::Denied),
        };
        let mut tx = self.history().begin_journal().await?;
        let session = self.catalog_session_in(&mut tx, bound_id).await?;
        let original = self
            .history()
            .get_default_session_in(&mut tx, &admission.route.origin())
            .await?
            .ok_or(SessionError::Denied)?;
        let mut context = SessionContext {
            lease: self.lease_for_in(&mut tx, bound_id).await?,
            cleanup_scope: None,
            user,
            identity: admission.identity,
            reply: admission.route,
            scope: admission.scope,
            session,
            default_id: original.id.clone(),
            selector_revision: None,
        };
        context.check(&original)?;
        context.check(context.session())?;
        if context.is_private() {
            if let Some(route) = find_in(&mut tx, &route_key(&context)?).await? {
                context.selector_revision = Some(route.revision);
                if context.session.kind == SessionKind::Temporary
                    || (route.session_id == bound_id.as_str() && route.return_session_id.is_some())
                {
                    context.cleanup_scope =
                        Some(cleanup_scope_for(context.reply_route(), bound_id));
                }
            }
        }
        tx.commit().await.map_err(DatabaseError::from)?;
        Ok(context)
    }
}
