use std::fmt::{Debug, Formatter, Result as FmtResult};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::Utc;
use rand::{random, rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};
use sqlx::{query, query_as, query_scalar};

use super::{
    claim::reconcile, AccountAuthority, AccountError, AccountGrant, AccountResult,
    LOGIN_TOKEN_PREFIX,
};

/// Only the authenticated HTTP adapter reveals the value once. Debug output is
/// deliberately redacted and this domain object is not serializable.
pub struct IssuedLoginToken {
    pub id: String,
    pub token: String,
    pub expires_at: i64,
}
impl Debug for IssuedLoginToken {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.debug_struct("IssuedLoginToken")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug)]
pub struct TelegramBinding {
    pub actor_id: String,
    pub chat_id: String,
    pub created_at: i64,
}

fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

impl AccountAuthority {
    pub async fn issue(&self, account: &str, connector: &str) -> AccountResult<IssuedLoginToken> {
        self.account(account).await?;
        let bot = self.bot(connector).ok_or(AccountError::BotUnavailable)?;
        self.rate(
            format!("issue:{account}:{connector}"),
            self.settings.issues_per_minute,
        )?;
        let mut bytes = [0_u8; 32];
        OsRng
            .try_fill_bytes(&mut bytes)
            .map_err(|_| AccountError::Entropy)?;
        let token = format!("{LOGIN_TOKEN_PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes));
        let id = format!("{:032x}", random::<u128>());
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let now = Utc::now().timestamp();
        let expires_at = now + self.settings.token_ttl_seconds as i64;
        let active: i64 = query_scalar("SELECT COUNT(*) FROM albert_login_accounts WHERE id=? AND disabled=0 AND must_change_password=0 AND canonical_user_id IS NOT NULL")
            .bind(account).fetch_one(&mut *tx).await?;
        if active != 1 {
            return Err(AccountError::Denied);
        }
        query("UPDATE albert_telegram_login_tokens SET revoked_at=? WHERE account_id=? AND connector_id=? AND consumed_at IS NULL AND revoked_at IS NULL")
            .bind(now).bind(account).bind(connector).execute(&mut *tx).await?;
        query("DELETE FROM albert_telegram_login_tokens WHERE expires_at<?")
            .bind(now - self.settings.token_retention_seconds as i64)
            .execute(&mut *tx)
            .await?;
        query("INSERT INTO albert_telegram_login_tokens(id,digest,account_id,connector_id,bot_id,created_at,expires_at) VALUES (?,?,?,?,?,?,?)")
            .bind(&id).bind(digest(&token)).bind(account).bind(connector).bind(&bot.bot_id).bind(now).bind(expires_at).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(IssuedLoginToken {
            id,
            token,
            expires_at,
        })
    }

    /// Called only with the transport-verified private actor, never a posted
    /// account ID or role. Consumption and history restoration share one commit.
    pub async fn login(
        &self,
        connector: &str,
        bot_id: u64,
        actor: i64,
        chat: i64,
        token: &str,
    ) -> AccountResult<AccountGrant> {
        self.rate(
            format!("login:{connector}:{bot_id}:{actor}"),
            self.settings.attempts_per_minute,
        )?;
        if actor <= 0
            || chat != actor
            || token.len() > 128
            || !token.starts_with(LOGIN_TOKEN_PREFIX)
        {
            return Err(AccountError::InvalidToken);
        }
        let bot = self.bot(connector).ok_or(AccountError::BotUnavailable)?;
        if bot.bot_id != bot_id.to_string() {
            return Err(AccountError::InvalidToken);
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let now = Utc::now().timestamp();
        let row: Option<(String, String, String)> = query_as("SELECT t.id,a.id,a.canonical_user_id FROM albert_telegram_login_tokens t JOIN albert_login_accounts a ON a.id=t.account_id WHERE t.digest=? AND t.connector_id=? AND t.bot_id=? AND t.consumed_at IS NULL AND t.revoked_at IS NULL AND t.expires_at>? AND a.disabled=0 AND a.must_change_password=0 AND a.canonical_user_id IS NOT NULL")
            .bind(digest(token)).bind(connector).bind(bot_id.to_string()).bind(now).fetch_optional(&mut *tx).await?;
        let (token_id, account, user) = row.ok_or(AccountError::InvalidToken)?;
        let existing: Option<String> = query_scalar("SELECT account_id FROM albert_telegram_bindings WHERE connector_id=? AND bot_id=? AND actor_id=? AND revoked_at IS NULL")
            .bind(connector).bind(bot_id.to_string()).bind(actor.to_string()).fetch_optional(&mut *tx).await?;
        if existing.as_ref().is_some_and(|id| id != &account) {
            return Err(AccountError::AlreadyBound);
        }
        reconcile(
            &self.history,
            &mut tx,
            &account,
            &user,
            connector,
            &actor.to_string(),
        )
        .await?;
        let generation = format!("{:032x}", random::<u128>());
        query("INSERT INTO albert_telegram_bindings(connector_id,bot_id,actor_id,account_id,chat_id,generation,created_at) VALUES (?,?,?,?,?,?,?) ON CONFLICT(connector_id,bot_id,actor_id) DO UPDATE SET account_id=excluded.account_id,chat_id=excluded.chat_id,generation=excluded.generation,created_at=excluded.created_at,revoked_at=NULL")
            .bind(connector).bind(bot_id.to_string()).bind(actor.to_string()).bind(&account).bind(chat.to_string()).bind(&generation).bind(now).execute(&mut *tx).await?;
        let consumed = query("UPDATE albert_telegram_login_tokens SET consumed_at=? WHERE id=? AND consumed_at IS NULL AND revoked_at IS NULL")
            .bind(now).bind(token_id).execute(&mut *tx).await?.rows_affected();
        if consumed != 1 {
            return Err(AccountError::InvalidToken);
        }
        tx.commit().await?;
        self.invalidate_binding(connector, Some(&actor.to_string()));
        self.actor(connector, bot_id, actor)
            .await?
            .ok_or(AccountError::Denied)
    }

    pub async fn logout(&self, connector: &str, bot: u64, actor: i64) -> AccountResult<()> {
        query("UPDATE albert_telegram_bindings SET revoked_at=? WHERE connector_id=? AND bot_id=? AND actor_id=? AND revoked_at IS NULL")
            .bind(Utc::now().timestamp()).bind(connector).bind(bot.to_string()).bind(actor.to_string()).execute(&self.pool).await?;
        self.invalidate_binding(connector, Some(&actor.to_string()));
        Ok(())
    }

    pub async fn bindings(
        &self,
        account: &str,
        connector: &str,
    ) -> AccountResult<Vec<TelegramBinding>> {
        self.account(account).await?;
        let rows: Vec<(String, String, i64)> = query_as("SELECT actor_id,chat_id,created_at FROM albert_telegram_bindings WHERE account_id=? AND connector_id=? AND revoked_at IS NULL ORDER BY created_at")
            .bind(account).bind(connector).fetch_all(&self.pool).await?;
        Ok(rows
            .into_iter()
            .map(|(actor_id, chat_id, created_at)| TelegramBinding {
                actor_id,
                chat_id,
                created_at,
            })
            .collect())
    }

    pub async fn revoke_token(&self, account: &str, connector: &str) -> AccountResult<()> {
        query("UPDATE albert_telegram_login_tokens SET revoked_at=? WHERE account_id=? AND connector_id=? AND consumed_at IS NULL AND revoked_at IS NULL")
            .bind(Utc::now().timestamp()).bind(account).bind(connector).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn disconnect(&self, account: &str, connector: &str) -> AccountResult<()> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let now = Utc::now().timestamp();
        query("UPDATE albert_telegram_bindings SET revoked_at=? WHERE account_id=? AND connector_id=? AND revoked_at IS NULL")
            .bind(now).bind(account).bind(connector).execute(&mut *tx).await?;
        query("UPDATE albert_telegram_login_tokens SET revoked_at=? WHERE account_id=? AND connector_id=? AND consumed_at IS NULL AND revoked_at IS NULL")
            .bind(now).bind(account).bind(connector).execute(&mut *tx).await?;
        tx.commit().await?;
        if let Ok(leases) = self.leases.lock() {
            for lease in leases.iter().filter_map(|state| state.upgrade()) {
                lease.revoke_account_binding(account, connector);
            }
        }
        Ok(())
    }
}
