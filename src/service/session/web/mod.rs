//! Explicit browser resource access. No selected channel chat or empty default
//! is created merely to prove access to a Session.
mod lifecycle;
mod metadata;

use octo_history::{SessionAccess, SessionId};
use sqlx::{Sqlite, Transaction};

use super::SessionService;
use crate::{
    db::{
        error::DatabaseError,
        repo::{account::native_identity_principal, identity::find_in},
    },
    sessions::{
        actor::ActorToolPolicy,
        error::{SessionError, SessionResult},
        AdmissionScope, ExternalIdentity, ReplyRoute, SessionContext, UserId,
    },
};

#[derive(Clone)]
pub(crate) struct WebActor {
    pub account_id: String,
    pub user_id: UserId,
    pub tools: ActorToolPolicy,
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use octo_history::{
        AppendRequest, CreateSession, HistoryStore, SessionAccess, SessionKind, SessionStore,
        SqliteHistory, Turn,
    };
    use sqlx::query_scalar;
    use tempfile::{tempdir, TempDir};

    use super::WebActor;
    use crate::{
        db::repo::identity::bind,
        service::session::SessionService,
        sessions::{
            actor::ActorToolPolicy, error::SessionError, settings::SessionSettings, UserId,
        },
    };

    async fn fixture() -> (TempDir, SessionService, WebActor, WebActor) {
        let dir = tempdir().unwrap();
        let history = Arc::new(
            SqliteHistory::open_retained(dir.path().join("history.db"))
                .await
                .unwrap(),
        );
        let service = SessionService::from_history(history, SessionSettings::default())
            .await
            .unwrap();
        let mut tx = service.history().begin_journal().await.unwrap();
        bind(&mut tx, "web", "account-one", "person-one")
            .await
            .unwrap();
        bind(&mut tx, "web", "account-two", "person-two")
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let first = WebActor {
            account_id: "account-one".into(),
            user_id: UserId::parse("person-one").unwrap(),
            tools: ActorToolPolicy::ChatOnly,
        };
        let second = WebActor {
            account_id: "account-two".into(),
            user_id: UserId::parse("person-two").unwrap(),
            tools: ActorToolPolicy::ExistingHost,
        };
        (dir, service, first, second)
    }
    #[tokio::test]
    async fn explicit_resource_context_never_creates_defaults_or_bypasses_private_group_access() {
        let (_dir, service, first, second) = fixture().await;
        let session = service.web_create(&first, "Own chat", false).await.unwrap();
        let context = service.web_context(&first, &session.id).await.unwrap();
        assert_eq!(context.tool_policy(), ActorToolPolicy::ChatOnly);
        assert_eq!(context.reply_route().channel(), "user:person-one");
        assert!(context.selector_revision().is_none());
        assert!(matches!(
            service.web_context(&second, &session.id).await,
            Err(SessionError::Denied)
        ));
        let mut group = CreateSession::private("");
        group.access = SessionAccess::GroupBound {
            connector: "telegram".into(),
            channel: "-42".into(),
        };
        let group = service.history().create_session(group).await.unwrap();
        assert!(matches!(
            service.web_context(&second, &group.id).await,
            Err(SessionError::Denied)
        ));
        assert_eq!(
            query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM history_session_defaults WHERE connector_key='web'",
            )
            .fetch_one(service.history().pool())
            .await
            .unwrap(),
            0
        );
        assert_eq!(
            query_scalar::<_, i64>("SELECT COUNT(*) FROM albert_private_session_routes")
                .fetch_one(service.history().pool())
                .await
                .unwrap(),
            0
        );
        let forged = WebActor {
            account_id: first.account_id.clone(),
            user_id: second.user_id.clone(),
            tools: ActorToolPolicy::ExistingHost,
        };
        assert!(matches!(
            service.web_context(&forged, &group.id).await,
            Err(SessionError::Denied)
        ));
    }
    #[tokio::test]
    async fn resource_temp_save_end_preserves_public_journal_and_never_writes_selector() {
        let (_dir, service, first, _) = fixture().await;
        let temp = service.web_create(&first, "Temporary", true).await.unwrap();
        let context = service.web_context(&first, &temp.id).await.unwrap();
        let request = AppendRequest {
            namespace: "web-test".into(),
            idempotency_key: "one".into(),
            digest: "one".into(),
            turns: vec![Turn::user("private input")],
        };
        let accepted = service
            .journal_router()
            .append_journal(&context.root_history_key(), &request)
            .await
            .unwrap();
        assert!(service
            .history()
            .get_session(&temp.id)
            .await
            .unwrap()
            .is_none());
        assert!(service
            .history()
            .journal_inventory()
            .await
            .unwrap()
            .iter()
            .all(|entry| entry.key != context.root_history_key()));
        let saved = service
            .save_temporary(&context, "save-once", None)
            .await
            .unwrap();
        assert_eq!(saved.session().kind, SessionKind::Permanent);
        let repeated = service
            .journal_router()
            .append_journal(&saved.root_history_key(), &request)
            .await
            .unwrap();
        assert_eq!(accepted.record_ids, repeated.record_ids);
        assert!(repeated.duplicate);
        assert!(
            !service
                .web_end(&first, &temp.id, saved.session().revision)
                .await
                .unwrap()
                .1
        );
        assert!(
            context.is_live(),
            "leaving a saved chat must not fence its ongoing work"
        );
        let discarded = service.web_create(&first, "Discarded", true).await.unwrap();
        let old = service.web_context(&first, &discarded.id).await.unwrap();
        assert!(
            service
                .web_end(&first, &discarded.id, discarded.revision)
                .await
                .unwrap()
                .1
        );
        assert!(!old.is_live());
        assert!(service.web_context(&first, &discarded.id).await.is_err());
        assert_eq!(
            query_scalar::<_, i64>("SELECT COUNT(*) FROM albert_private_session_routes")
                .fetch_one(service.history().pool())
                .await
                .unwrap(),
            0
        );
    }
}

pub(super) async fn verify_actor(
    tx: &mut Transaction<'_, Sqlite>,
    actor: &WebActor,
) -> SessionResult<()> {
    if find_in(tx, "web", &actor.account_id).await?.as_deref() != Some(actor.user_id.as_str()) {
        return Err(SessionError::Denied);
    }
    Ok(())
}
impl SessionService {
    pub async fn web_sql_principal(&self, user: &UserId) -> SessionResult<String> {
        native_identity_principal(self.database.pool(), user.as_str())
            .await
            .map_err(|error| DatabaseError::from(error).into())
    }
    pub async fn web_context(
        &self,
        actor: &WebActor,
        id: &SessionId,
    ) -> SessionResult<SessionContext> {
        let mut tx = self.history.begin_journal().await?;
        verify_actor(&mut tx, actor).await?;
        let session = self.catalog_session_in(&mut tx, id).await?;
        if session.access
            != (SessionAccess::Private {
                owner: actor.user_id.as_str().into(),
            })
        {
            return Err(SessionError::Denied);
        }
        let lease = self.lease_for_in(&mut tx, id).await?;
        let context = SessionContext {
            actor_tools: Some(actor.tools),
            lease,
            cleanup_scope: None,
            user: Some(actor.user_id.clone()),
            identity: Some(ExternalIdentity::new("web", &actor.account_id)?),
            reply: ReplyRoute::new("web", &format!("user:{}", actor.user_id.as_str()), None)?,
            scope: AdmissionScope::Private,
            default_id: id.clone(),
            selector_revision: None,
            session,
        };
        context.check(context.session())?;
        tx.commit().await.map_err(DatabaseError::from)?;
        Ok(context)
    }

    /// A server-stamped scheduler route proves the same private owner without
    /// manufacturing a separate default Session for the browser transport.
    pub async fn web_reminder_context(
        &self,
        id: &SessionId,
        reply: ReplyRoute,
    ) -> SessionResult<SessionContext> {
        let user = reply
            .channel()
            .strip_prefix("user:")
            .ok_or(SessionError::Denied)?;
        let user = UserId::parse(user)?;
        if reply.connector() != "web" {
            return Err(SessionError::Denied);
        }
        let mut tx = self.history.begin_journal().await?;
        let session = self.catalog_session_in(&mut tx, id).await?;
        if session.access
            != (SessionAccess::Private {
                owner: user.as_str().into(),
            })
        {
            return Err(SessionError::Denied);
        }
        let context = SessionContext {
            actor_tools: Some(ActorToolPolicy::ExistingHost),
            lease: self.lease_for_in(&mut tx, id).await?,
            cleanup_scope: None,
            user: Some(user),
            identity: None,
            reply,
            scope: AdmissionScope::Private,
            default_id: id.clone(),
            selector_revision: None,
            session,
        };
        context.check(context.session())?;
        tx.commit().await.map_err(DatabaseError::from)?;
        Ok(context)
    }
}
