use crate::{
    db::{error::DatabaseError, repo::route::find_in},
    service::session::{route_key, SessionService},
    sessions::{
        error::{SessionError, SessionResult},
        Admission, AdmissionScope, SessionContext,
    },
};

impl SessionService {
    /// Explicit navigation remains possible after RAM expiry. The fallback
    /// proves the actor against the immutable private default, never by guessing
    /// an owner for a lost temporary journal.
    pub async fn control_context(&self, admission: Admission) -> SessionResult<SessionContext> {
        match self.admit(admission.clone()).await {
            Ok(context) => return Ok(context),
            Err(SessionError::Expired | SessionError::Ended) => {}
            Err(error) => return Err(error),
        }
        if admission.scope != AdmissionScope::Private {
            return Err(SessionError::GroupSelector);
        }
        let identity = admission.identity.as_ref().ok_or(SessionError::Denied)?;
        let user = self.resolve_identity(identity).await?;
        let mut tx = self.history.begin_journal().await?;
        let default = self
            .history
            .get_default_session_in(&mut tx, &admission.route.origin())
            .await?
            .ok_or(SessionError::NotFound)?;
        let mut context = SessionContext {
            lease: self.lease_for_in(&mut tx, &default.id).await?,
            cleanup_scope: None,
            user: Some(user),
            identity: admission.identity,
            reply: admission.route,
            scope: AdmissionScope::Private,
            session: default.clone(),
            default_id: default.id,
            selector_revision: None,
        };
        context.check(&context.session)?;
        context.selector_revision = find_in(&mut tx, &route_key(&context)?)
            .await?
            .map(|route| route.revision);
        tx.commit().await.map_err(DatabaseError::from)?;
        Ok(context)
    }
}
