use octo_history::{SessionAccess, SessionId};
use sqlx::{Sqlite, Transaction};

use super::{route_key, SessionService};
use crate::{
    db::{
        error::DatabaseError,
        repo::{
            identity::{bind, find_in as find_identity},
            route::{find_in as find_route, set},
        },
    },
    sessions::{
        cleanup_scope_for,
        error::{SessionError, SessionResult},
        require_host, Admission, AdmissionScope, ExternalIdentity, SessionContext, UserId,
    },
};

impl SessionService {
    /// The adapter must verify the identity before constructing Admission. Roles
    /// are deliberately absent: two external owners remain two canonical users.
    pub async fn admit(&self, admission: Admission) -> SessionResult<SessionContext> {
        require_host(&admission.host)?;
        let mut tx = self.history.begin_journal().await?;
        let user = match admission.identity.as_ref() {
            Some(identity) => Some(self.resolve_identity_in(&mut tx, identity).await?),
            None if admission.scope == AdmissionScope::Group => None,
            None => return Err(SessionError::Denied),
        };
        let origin = admission.route.origin();
        let access = match admission.scope {
            AdmissionScope::Private => SessionAccess::Private {
                owner: user.as_ref().ok_or(SessionError::Denied)?.as_str().into(),
            },
            AdmissionScope::Group => SessionAccess::GroupBound {
                connector: origin.connector.clone(),
                channel: origin.channel.clone(),
            },
        };
        let default = self
            .ensure_migrated_default_in(&mut tx, &origin, access.clone(), None)
            .await?;
        // A previously bound private origin cannot quietly change its owner or
        // be reinterpreted as a group when transport configuration changes.
        if default.access != access {
            return Err(SessionError::Denied);
        }
        let mut context = SessionContext {
            actor_tools: None,
            lease: self.lease_for_in(&mut tx, &default.id).await?,
            cleanup_scope: None,
            user,
            identity: admission.identity,
            reply: admission.route,
            scope: admission.scope,
            default_id: default.id.clone(),
            session: default,
            selector_revision: None,
        };
        context.check(&context.session)?;
        if context.is_private() {
            let key = route_key(&context)?;
            if let Some(route) = find_route(&mut tx, &key).await? {
                let id = SessionId::parse(&route.session_id)?;
                let selected = self.catalog_session_in(&mut tx, &id).await?;
                context.lease = self.lease_for_in(&mut tx, &id).await?;
                context.check(&selected)?;
                context.session = selected;
                context.selector_revision = Some(route.revision);
                if route.return_session_id.is_some() {
                    context.cleanup_scope = Some(cleanup_scope_for(&context.reply, &id));
                }
            } else {
                let revision =
                    set(&mut tx, &key, context.session_id().as_str(), None, None).await?;
                context.selector_revision = Some(revision);
            }
        }
        tx.commit().await.map_err(DatabaseError::from)?;
        Ok(context)
    }

    pub async fn resolve_identity(&self, identity: &ExternalIdentity) -> SessionResult<UserId> {
        let mut tx = self.history.begin_journal().await?;
        let user = self.resolve_identity_in(&mut tx, identity).await?;
        tx.commit().await.map_err(DatabaseError::from)?;
        Ok(user)
    }

    pub(super) async fn resolve_identity_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        identity: &ExternalIdentity,
    ) -> SessionResult<UserId> {
        let configured = self
            .settings
            .identity_links
            .iter()
            .find(|link| link.connector == identity.connector() && link.actor == identity.actor())
            .map(|link| UserId::parse(&link.user_id))
            .transpose()?;
        if let Some(stored) = find_identity(tx, identity.connector(), identity.actor()).await? {
            let stored = UserId::parse(&stored)?;
            if configured
                .as_ref()
                .is_some_and(|configured| configured != &stored)
            {
                return Err(DatabaseError::IdentityConflict.into());
            }
            return Ok(stored);
        }
        let user = configured.unwrap_or_else(|| UserId::for_identity(identity));
        bind(tx, identity.connector(), identity.actor(), user.as_str()).await?;
        Ok(user)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use octo_history::{SessionOrigin, SessionStore, SqliteHistory};
    use sqlx::query;
    use tempfile::tempdir;

    use crate::{
        service::session::SessionService,
        sessions::{
            error::{SessionError, SessionResult},
            settings::SessionSettings,
            Admission, AdmissionScope, ExternalIdentity, ReplyRoute,
        },
    };

    #[tokio::test]
    async fn metadata_read_errors_propagate_and_rollback_default_creation() -> SessionResult<()> {
        let directory = tempdir().unwrap();
        let history =
            Arc::new(SqliteHistory::open_retained(directory.path().join("sessions.db")).await?);
        let service =
            SessionService::from_history(history.clone(), SessionSettings::default()).await?;
        query("DROP TABLE albert_private_session_routes")
            .execute(history.pool())
            .await
            .unwrap();
        let route = ReplyRoute::new("console", "console", None)?;
        let admission = Admission::new(
            ExternalIdentity::new("console", "local")?,
            route,
            AdmissionScope::Private,
            "host",
        )?;
        assert!(matches!(
            service.admit(admission).await,
            Err(SessionError::Database(_))
        ));
        let origin = SessionOrigin {
            connector: "console".into(),
            channel: "console".into(),
        };
        assert!(history.get_default_session(&origin).await?.is_none());
        Ok(())
    }
}
