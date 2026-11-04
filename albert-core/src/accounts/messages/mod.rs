//! Durable provenance for queued work, separate from model-supplied messages.
use serde::{Deserialize, Serialize};
use serde_json::{from_str, to_string};
use sqlx::{query, query_scalar, Sqlite, Transaction};

use super::{AccountAuthority, AccountGrant};
use crate::{
    messaging::{MessagingError, MessagingResult},
    sessions::SessionContext,
};

#[derive(Serialize, Deserialize)]
struct Provenance {
    account_id: String,
    user_id: String,
    owner: bool,
    binding: Option<(String, String, String, String)>,
}

pub(crate) async fn record_message_authority(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    context: &SessionContext,
) -> MessagingResult<()> {
    let Some(grant) = context.account() else {
        return Ok(());
    };
    if !grant.valid_in(tx).await {
        return Err(MessagingError::Denied);
    }
    let stored = Provenance {
        account_id: grant.account_id.clone(),
        user_id: grant.user_id.clone(),
        owner: grant.role.is_owner(),
        binding: grant.binding.as_ref().map(|b| {
            (
                b.connector.clone(),
                b.bot_id.clone(),
                b.actor_id.clone(),
                b.generation.clone(),
            )
        }),
    };
    query("UPDATE albert_messages SET account_authority=? WHERE message_id=?")
        .bind(to_string(&stored)?)
        .bind(id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

impl AccountAuthority {
    pub(crate) async fn message_context(
        &self,
        id: &str,
        context: SessionContext,
    ) -> MessagingResult<SessionContext> {
        let stored: Option<String> =
            query_scalar("SELECT account_authority FROM albert_messages WHERE message_id=?")
                .bind(id)
                .fetch_one(&self.pool)
                .await?;
        // Never turn a legacy journal identity into newly acquired account rights.
        let Some(stored) = stored else {
            return Ok(context);
        };
        let stored: Provenance = from_str(&stored)?;
        let grant: AccountGrant = if let Some((connector, bot, actor, generation)) = &stored.binding
        {
            let grant = self
                .actor(
                    connector,
                    bot.parse().map_err(|_| MessagingError::Denied)?,
                    actor.parse().map_err(|_| MessagingError::Denied)?,
                )
                .await
                .map_err(|_| MessagingError::Denied)?
                .ok_or(MessagingError::Denied)?;
            if grant
                .binding
                .as_ref()
                .is_none_or(|b| b.generation != *generation)
            {
                return Err(MessagingError::Denied);
            }
            grant
        } else {
            self.account(&stored.account_id)
                .await
                .map_err(|_| MessagingError::Denied)?
        };
        if grant.account_id != stored.account_id
            || grant.user_id != stored.user_id
            || grant.role.is_owner() != stored.owner
            || context
                .user_id()
                .is_none_or(|user| user.as_str() != grant.user_id)
            || !grant.valid().await
        {
            return Err(MessagingError::Denied);
        }
        Ok(context.with_account(grant))
    }
}

#[cfg(test)]
mod tests {
    use sqlx::query;

    use super::{record_message_authority, AccountAuthority};
    use crate::{
        accounts::tests::fixture,
        service::session::SessionService,
        sessions::{
            settings::SessionSettings, Admission, AdmissionScope, ExternalIdentity, ReplyRoute,
        },
    };

    #[tokio::test]
    async fn queued_work_keeps_its_original_account_and_binding_after_restart() {
        let (_dir, accounts) = fixture().await;
        let service =
            SessionService::from_history(accounts.history.clone(), SessionSettings::default())
                .await
                .unwrap();
        let issued = accounts.issue("account", "telegram").await.unwrap();
        let grant = accounts
            .login("telegram", 99, 7, 7, &issued.token)
            .await
            .unwrap();
        let source = service
            .admit(
                Admission::new(
                    ExternalIdentity::new("telegram", "7").unwrap(),
                    ReplyRoute::new("telegram", "7", None).unwrap(),
                    AdmissionScope::Private,
                    "host",
                )
                .unwrap()
                .with_account(grant)
                .unwrap(),
            )
            .await
            .unwrap();
        let web = source
            .clone()
            .with_account(accounts.account("account").await.unwrap());
        let mut legacy = source.clone();
        legacy.account = None;
        let mut tx = accounts.pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        for (id, context) in [("native", &source), ("browser", &web), ("legacy", &legacy)] {
            query("INSERT INTO albert_messages(message_id,actor_id,source_session,source_record,envelope,digest,created_at,deadline_unix_ms) VALUES (?,'profile',?,1,'{}','fixture',0,9999999999999)")
                .bind(id).bind(source.session_id().as_str()).execute(&mut *tx).await.unwrap();
            record_message_authority(&mut tx, id, context)
                .await
                .unwrap();
        }
        tx.commit().await.unwrap();
        let restarted = AccountAuthority::new(accounts.history.clone(), Default::default());
        restarted
            .register_bot("telegram", 99, "fixture_bot")
            .await
            .unwrap();
        assert!(restarted
            .message_context("native", legacy.clone())
            .await
            .unwrap()
            .account()
            .is_some());
        restarted.logout("telegram", 99, 7).await.unwrap();
        assert!(restarted
            .message_context("native", legacy.clone())
            .await
            .is_err());
        assert!(restarted
            .message_context("browser", legacy.clone())
            .await
            .unwrap()
            .account()
            .is_some());
        assert!(restarted
            .message_context("legacy", legacy.clone())
            .await
            .unwrap()
            .account()
            .is_none());
        query("UPDATE albert_login_accounts SET role='user' WHERE id='account'")
            .execute(&accounts.pool)
            .await
            .unwrap();
        assert!(restarted.message_context("browser", legacy).await.is_err());
    }
}
