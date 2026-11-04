//! Transport-neutral account authority and Telegram pairing. Browser JWTs and
//! password authentication remain at the web edge.
mod claim;
mod grants;
mod messages;
mod settings;
mod tokens;

use std::{
    collections::{HashMap, HashSet},
    fmt::{Debug, Formatter, Result as FmtResult},
    sync::{Arc, Mutex, RwLock, Weak},
    time::Instant,
};

use chrono::Utc;
use octo_history::{HistoryError, SqliteHistory};
use sqlx::{query, Error as SqlxError, SqlitePool};
use thiserror::Error;

pub(crate) use grants::GrantState;
pub use grants::{AccountGrant, AccountRole, BindingScope};
pub(crate) use messages::record_message_authority;
pub use settings::TelegramLoginSettings;
pub use tokens::{IssuedLoginToken, TelegramBinding};

pub const LOGIN_TOKEN_PREFIX: &str = "albert-tg-";

#[derive(Debug, Error)]
pub enum AccountError {
    #[error("Sign in with a current account before continuing.")]
    Denied,
    #[error("The login token is invalid or expired. Create a new token in the panel.")]
    InvalidToken,
    #[error("Telegram is not connected. Ask the owner to configure the connector.")]
    BotUnavailable,
    #[error("Too many attempts. Please try again later.")]
    RateLimited,
    #[error("A login token could not be created. Please try again.")]
    Entropy,
    #[error("This Telegram identity is already connected to another account.")]
    AlreadyBound,
    #[error("Telegram authentication is temporarily unavailable.")]
    Database(#[from] SqlxError),
    #[error("Telegram history could not be restored safely.")]
    History(#[from] HistoryError),
}
pub type AccountResult<T> = Result<T, AccountError>;

#[derive(Clone, Debug)]
pub struct TelegramBot {
    pub connector: String,
    pub bot_id: String,
    pub username: String,
}

#[derive(Clone)]
pub struct AccountAuthority {
    pool: SqlitePool,
    history: Arc<SqliteHistory>,
    settings: TelegramLoginSettings,
    bots: Arc<RwLock<HashMap<String, TelegramBot>>>,
    restarting: Arc<RwLock<HashSet<String>>>,
    limits: Arc<Mutex<HashMap<String, (Instant, usize)>>>,
    leases: Arc<Mutex<Vec<Weak<grants::GrantState>>>>,
}

impl Debug for AccountAuthority {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.debug_struct("AccountAuthority").finish_non_exhaustive()
    }
}

impl AccountAuthority {
    pub fn new(history: Arc<SqliteHistory>, settings: TelegramLoginSettings) -> Self {
        Self {
            pool: history.pool().clone(),
            history,
            settings,
            bots: Default::default(),
            restarting: Default::default(),
            limits: Default::default(),
            leases: Default::default(),
        }
    }

    pub fn bot(&self, connector: &str) -> Option<TelegramBot> {
        self.bots.read().ok()?.get(connector).cloned()
    }

    pub fn bots(&self) -> Vec<TelegramBot> {
        self.bots
            .read()
            .map(|bots| bots.values().cloned().collect())
            .unwrap_or_default()
    }

    pub fn requires_restart(&self, connector: &str) -> bool {
        self.restarting
            .read()
            .is_ok_and(|set| set.contains(connector))
    }

    pub fn mark_restarting(&self, connector: &str) {
        if let Ok(mut set) = self.restarting.write() {
            set.insert(connector.into());
        }
        if let Ok(mut bots) = self.bots.write() {
            bots.remove(connector);
        }
        self.invalidate_binding(connector, None);
    }

    pub async fn register_bot(
        &self,
        connector: &str,
        bot_id: u64,
        username: &str,
    ) -> AccountResult<()> {
        if connector.is_empty() || connector.len() > 128 || bot_id == 0 || username.len() > 128 {
            return Err(AccountError::BotUnavailable);
        }
        let bot_id = bot_id.to_string();
        let now = Utc::now().timestamp();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        query("UPDATE albert_telegram_login_tokens SET revoked_at=? WHERE connector_id=? AND bot_id<>? AND revoked_at IS NULL")
            .bind(now).bind(connector).bind(&bot_id).execute(&mut *tx).await?;
        query("UPDATE albert_telegram_bindings SET revoked_at=? WHERE connector_id=? AND bot_id<>? AND revoked_at IS NULL")
            .bind(now).bind(connector).bind(&bot_id).execute(&mut *tx).await?;
        query("INSERT INTO albert_telegram_bots(connector_id,bot_id,username) VALUES (?,?,?) ON CONFLICT(connector_id) DO UPDATE SET bot_id=excluded.bot_id,username=excluded.username")
            .bind(connector).bind(&bot_id).bind(username).execute(&mut *tx).await?;
        tx.commit().await?;
        let changed = self.bot(connector).is_some_and(|old| old.bot_id != bot_id);
        if changed {
            self.invalidate_binding(connector, None);
        }
        let bot = TelegramBot {
            connector: connector.into(),
            bot_id,
            username: username.into(),
        };
        if let Ok(mut bots) = self.bots.write() {
            bots.insert(connector.into(), bot);
        }
        if let Ok(mut set) = self.restarting.write() {
            set.remove(connector);
        }
        Ok(())
    }

    fn rate(&self, key: String, max: usize) -> AccountResult<()> {
        let mut limits = self.limits.lock().map_err(|_| AccountError::RateLimited)?;
        limits.retain(|_, (at, _)| at.elapsed().as_secs() < 60);
        if !limits.contains_key(&key) && limits.len() >= self.settings.max_rate_limit_keys {
            return Err(AccountError::RateLimited);
        }
        let entry = limits.entry(key).or_insert((Instant::now(), 0));
        if entry.1 >= max {
            return Err(AccountError::RateLimited);
        }
        entry.1 += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use octo_history::SqliteHistory;
    use sqlx::{query, query_scalar};
    use tempfile::{tempdir, TempDir};

    use super::{AccountAuthority, AccountError, AccountRole, TelegramLoginSettings};
    use crate::db::Database;

    pub(super) async fn fixture() -> (TempDir, AccountAuthority) {
        let dir = tempdir().unwrap();
        let history = Arc::new(
            SqliteHistory::open_retained(dir.path().join("history.sqlite"))
                .await
                .unwrap(),
        );
        Database::from_pool(history.pool().clone()).await.unwrap();
        query("INSERT INTO albert_users(id) VALUES ('profile'),('legacy')")
            .execute(history.pool())
            .await
            .unwrap();
        query("INSERT INTO albert_login_accounts(id,username,password_hash,role,canonical_user_id,must_change_password) VALUES ('account','fixture','test-hash','owner','profile',0)").execute(history.pool()).await.unwrap();
        query("INSERT INTO albert_identity_bindings VALUES ('telegram','7','legacy')")
            .execute(history.pool())
            .await
            .unwrap();
        let accounts = AccountAuthority::new(history, TelegramLoginSettings::default());
        accounts
            .register_bot("telegram", 99, "fixture_bot")
            .await
            .unwrap();
        (dir, accounts)
    }

    #[tokio::test]
    async fn pairing_is_single_use_scoped_hashed_and_persists_without_relogin() {
        let (_dir, accounts) = fixture().await;
        let token = accounts.issue("account", "telegram").await.unwrap();
        let stored: String = query_scalar("SELECT digest FROM albert_telegram_login_tokens")
            .fetch_one(&accounts.pool)
            .await
            .unwrap();
        assert_ne!(stored, token.token);
        assert!(!format!("{token:?}").contains(&token.token));
        assert!(accounts
            .login("telegram", 100, 7, 7, &token.token)
            .await
            .is_err());
        let web = accounts.account("account").await.unwrap();
        let grant = accounts
            .login("telegram", 99, 7, 7, &token.token)
            .await
            .unwrap();
        assert_eq!(grant.role, AccountRole::Owner);
        assert!(grant.valid().await);
        assert!(matches!(
            accounts.login("telegram", 99, 7, 7, &token.token).await,
            Err(AccountError::InvalidToken)
        ));
        let reopened =
            AccountAuthority::new(accounts.history.clone(), TelegramLoginSettings::default());
        reopened
            .register_bot("telegram", 99, "fixture_bot")
            .await
            .unwrap();
        assert_eq!(
            reopened
                .actor("telegram", 99, 7)
                .await
                .unwrap()
                .unwrap()
                .user_id,
            "profile"
        );
        accounts.logout("telegram", 99, 7).await.unwrap();
        assert!(!grant.is_live());
        assert!(web.is_live());
        assert!(web.valid().await);
        assert!(accounts.actor("telegram", 99, 7).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn reissue_expiry_races_and_other_accounts_cannot_take_history() {
        let (_dir, accounts) = fixture().await;
        let first = accounts.issue("account", "telegram").await.unwrap();
        let second = accounts.issue("account", "telegram").await.unwrap();
        assert!(accounts
            .login("telegram", 99, 7, 7, &first.token)
            .await
            .is_err());
        let (left, right) = tokio::join!(
            accounts.login("telegram", 99, 7, 7, &second.token),
            accounts.login("telegram", 99, 7, 7, &second.token)
        );
        assert_ne!(left.is_ok(), right.is_ok());
        let token = accounts.issue("account", "telegram").await.unwrap();
        query("UPDATE albert_telegram_login_tokens SET expires_at=0 WHERE id=?")
            .bind(&token.id)
            .execute(&accounts.pool)
            .await
            .unwrap();
        assert!(accounts
            .login("telegram", 99, 7, 7, &token.token)
            .await
            .is_err());
        accounts.logout("telegram", 99, 7).await.unwrap();
        query("INSERT INTO albert_login_accounts(id,username,password_hash,role,canonical_user_id,must_change_password) VALUES ('other','other','test-hash','user','legacy',0)").execute(&accounts.pool).await.unwrap();
        query("UPDATE albert_identity_bindings SET user_id='legacy' WHERE connector_id='telegram'")
            .execute(&accounts.pool)
            .await
            .unwrap();
        let token = accounts.issue("account", "telegram").await.unwrap();
        assert!(matches!(
            accounts.login("telegram", 99, 7, 7, &token.token).await,
            Err(AccountError::AlreadyBound)
        ));
        let consumed: Option<i64> =
            query_scalar("SELECT consumed_at FROM albert_telegram_login_tokens WHERE id=?")
                .bind(token.id)
                .fetch_one(&accounts.pool)
                .await
                .unwrap();
        assert_eq!(consumed, None);
    }
    #[tokio::test]
    async fn target_profile_is_exclusive_even_without_legacy_identity() {
        let (_dir, accounts) = fixture().await;
        query("INSERT INTO albert_login_accounts(id,username,password_hash,role,canonical_user_id,must_change_password) VALUES ('other','other','test-hash','user','profile',0)").execute(&accounts.pool).await.unwrap();
        let token = accounts.issue("account", "telegram").await.unwrap();
        assert!(matches!(
            accounts.login("telegram", 99, 8, 8, &token.token).await,
            Err(AccountError::AlreadyBound)
        ));
        assert!(accounts.actor("telegram", 99, 8).await.unwrap().is_none());
        query(
            "UPDATE albert_identity_bindings SET user_id='profile' WHERE connector_id='telegram'",
        )
        .execute(&accounts.pool)
        .await
        .unwrap();
        assert!(matches!(
            accounts.login("telegram", 99, 7, 7, &token.token).await,
            Err(AccountError::AlreadyBound)
        ));
        let used: Option<i64> =
            query_scalar("SELECT consumed_at FROM albert_telegram_login_tokens WHERE id=?")
                .bind(token.id)
                .fetch_one(&accounts.pool)
                .await
                .unwrap();
        assert!(used.is_none());
    }

    #[tokio::test]
    async fn disconnect_revokes_unused_tokens_and_pending_scopes_but_preserves_web_access() {
        use crate::{
            service::session::SessionService,
            sessions::{
                settings::SessionSettings, Admission, AdmissionScope, ExternalIdentity, ReplyRoute,
            },
        };
        use octo_approval::ScopeValidator;
        let (_dir, accounts) = fixture().await;
        let service =
            SessionService::from_history(accounts.history.clone(), SessionSettings::default())
                .await
                .unwrap();
        let token = accounts.issue("account", "telegram").await.unwrap();
        let tg = accounts
            .login("telegram", 99, 7, 7, &token.token)
            .await
            .unwrap();
        let context = service
            .admit(
                Admission::new(
                    ExternalIdentity::new("telegram", "7").unwrap(),
                    ReplyRoute::new("telegram", "7", None).unwrap(),
                    AdmissionScope::Private,
                    "host",
                )
                .unwrap()
                .with_account(tg.clone())
                .unwrap(),
            )
            .await
            .unwrap();
        let registry = service.scope_registry();
        let scope = registry.binding(&context).unwrap().unwrap();
        assert!(ScopeValidator::allows(registry.as_ref(), &scope));
        let web = accounts.account("account").await.unwrap();
        let web_context = context.clone().with_account(web.clone());
        let web_scope = registry.binding(&web_context).unwrap().unwrap();
        let pending = accounts.issue("account", "telegram").await.unwrap();
        drop(context);
        drop(tg);
        // Pending approvals retain the original proof after its turn ends.
        assert!(ScopeValidator::allows(registry.as_ref(), &scope));
        accounts.disconnect("account", "telegram").await.unwrap();
        assert!(!ScopeValidator::allows(registry.as_ref(), &scope));
        assert!(ScopeValidator::allows(registry.as_ref(), &web_scope));
        assert!(web.valid().await);
        assert!(accounts
            .login("telegram", 99, 7, 7, &pending.token)
            .await
            .is_err());
        accounts.invalidate_account("account");
        assert!(!ScopeValidator::allows(registry.as_ref(), &web_scope));
    }

    #[tokio::test]
    async fn login_expiry_is_checked_after_the_writer_lock() {
        use chrono::Utc;
        use std::time::Duration;
        use tokio::{
            spawn,
            time::{sleep, timeout},
        };
        let (_dir, accounts) = fixture().await;
        let token = accounts.issue("account", "telegram").await.unwrap();
        let expires = Utc::now().timestamp() + 1;
        query("UPDATE albert_telegram_login_tokens SET expires_at=? WHERE id=?")
            .bind(expires)
            .bind(&token.id)
            .execute(&accounts.pool)
            .await
            .unwrap();
        let mut lock = accounts.pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
        let other = accounts.clone();
        let login = spawn(async move { other.login("telegram", 99, 7, 7, &token.token).await });
        sleep(Duration::from_millis(100)).await;
        timeout(Duration::from_secs(3), async {
            while Utc::now().timestamp() <= expires {
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        query("SELECT 1").execute(&mut *lock).await.unwrap();
        lock.commit().await.unwrap();
        assert!(matches!(
            timeout(Duration::from_secs(3), login)
                .await
                .unwrap()
                .unwrap(),
            Err(AccountError::InvalidToken)
        ));
    }
}
