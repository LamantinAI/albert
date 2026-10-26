//! Scheduler admission uses the original immutable channel default to verify
//! access. It never consults the user's currently selected private conversation.

use octo_history::{SessionAccess, SessionId};

use super::SessionService;
use crate::db::error::DatabaseError;
use crate::sessions::{
    error::{SessionError, SessionResult},
    require_host, AdmissionScope, ReplyRoute, SessionContext, UserId,
};

impl SessionService {
    /// Trusted scheduler adapter only: creation pins the Session and real
    /// ReplyRoute. Neither field may be overridden by model-supplied alarm data.
    pub async fn context_for_reminder(
        &self,
        session_id: &SessionId,
        reply: ReplyRoute,
    ) -> SessionResult<SessionContext> {
        let mut tx = self.history().begin_journal().await?;
        let session = self.catalog_session_in(&mut tx, session_id).await?;
        require_host(&session.host)?;
        let original = self
            .history()
            .get_default_session_in(&mut tx, &reply.origin())
            .await?
            .ok_or(SessionError::Denied)?;
        let (scope, user) = match (&session.access, &original.access) {
            (SessionAccess::Private { owner }, SessionAccess::Private { owner: route_owner })
                if owner == route_owner =>
            {
                (AdmissionScope::Private, Some(UserId::parse(owner)?))
            }
            (
                SessionAccess::GroupBound { connector, channel },
                SessionAccess::GroupBound { .. },
            ) if connector == reply.connector()
                && channel == reply.channel()
                && original.id == session.id =>
            {
                (AdmissionScope::Group, None)
            }
            _ => return Err(SessionError::Denied),
        };
        let context = SessionContext {
            lease: self.lease_for_in(&mut tx, session_id).await?,
            cleanup_scope: None,
            user,
            identity: None,
            reply,
            scope,
            session,
            default_id: original.id,
            selector_revision: None,
        };
        context.check(context.session())?;
        tx.commit().await.map_err(DatabaseError::from)?;
        Ok(context)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use octo_history::{SessionStore, SessionUpdate, SqliteHistory};
    use tempfile::{tempdir, TempDir};

    use crate::{
        service::session::SessionService,
        sessions::{
            settings::SessionSettings, Admission, AdmissionScope, ExternalIdentity, ReplyRoute,
        },
    };

    async fn fixture() -> (TempDir, SessionService) {
        let directory = tempdir().unwrap();
        let history = Arc::new(
            SqliteHistory::open_retained(directory.path().join("history.sqlite"))
                .await
                .unwrap(),
        );
        let service = SessionService::from_history(history, SessionSettings::default())
            .await
            .unwrap();
        (directory, service)
    }

    fn private(channel: &str, actor: &str) -> Admission {
        Admission::new(
            ExternalIdentity::new("telegram", actor).unwrap(),
            ReplyRoute::new("telegram", channel, None).unwrap(),
            AdmissionScope::Private,
            "host",
        )
        .unwrap()
    }

    #[tokio::test]
    async fn old_reminder_uses_original_session_after_selector_switch_and_archive() {
        let (_directory, service) = fixture().await;
        let original = service.admit(private("7", "7")).await.unwrap();
        let current = service
            .create_private(&original, "Another chat")
            .await
            .unwrap();
        assert_ne!(original.session_id(), current.session_id());
        service
            .history()
            .update_session(
                original.session_id(),
                SessionUpdate {
                    expected_revision: original.session().revision,
                    title: None,
                    archived: Some(true),
                },
            )
            .await
            .unwrap();
        let reminder = service
            .context_for_reminder(original.session_id(), original.reply_route().clone())
            .await
            .unwrap();
        assert_eq!(reminder.session_id(), original.session_id());
        assert_eq!(reminder.reply_route().channel(), "7");
        assert!(reminder.identity().is_none());
        assert!(reminder.selector_revision().is_none());
    }

    #[tokio::test]
    async fn reminder_route_does_not_grant_another_users_or_groups_context() {
        let (_directory, service) = fixture().await;
        let owner = service.admit(private("7", "7")).await.unwrap();
        let other = service.admit(private("8", "8")).await.unwrap();
        assert!(service
            .context_for_reminder(owner.session_id(), other.reply_route().clone())
            .await
            .is_err());
        let group = service
            .admit(
                Admission::group(
                    None,
                    ReplyRoute::new("telegram", "-42", None).unwrap(),
                    "host",
                )
                .unwrap(),
            )
            .await
            .unwrap();
        assert!(service
            .context_for_reminder(group.session_id(), owner.reply_route().clone())
            .await
            .is_err());
        assert!(service
            .context_for_reminder(owner.session_id(), group.reply_route().clone())
            .await
            .is_err());
        assert!(service
            .context_for_reminder(group.session_id(), group.reply_route().clone())
            .await
            .is_ok());
    }
}
