use std::{
    fmt::{Debug, Formatter, Result as FmtResult},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Weak,
    },
    time::Instant,
};

use octo_history::SessionId;
use sqlx::{query_as, Sqlite, Transaction};
use tokio::sync::watch::{channel, Sender};

use super::{AccountAuthority, AccountError, AccountResult};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountRole {
    Owner,
    User,
}

impl AccountRole {
    pub(crate) fn is_owner(self) -> bool {
        self == Self::Owner
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindingScope {
    pub connector: String,
    pub bot_id: String,
    pub actor_id: String,
    pub generation: String,
}

pub(crate) struct GrantState {
    pub(crate) id: String,
    pub(crate) created: Instant,
    cancelled: Sender<bool>,
    account_id: String,
    binding: Option<BindingScope>,
    live: AtomicBool,
}

#[derive(Clone, Debug)]
pub struct AccountGrant {
    pub account_id: String,
    pub user_id: String,
    pub role: AccountRole,
    pub binding: Option<BindingScope>,
    authority: AccountAuthority,
    state: Arc<GrantState>,
}

impl Debug for GrantState {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.debug_struct("GrantState").finish_non_exhaustive()
    }
}
impl GrantState {
    pub(crate) fn is_live(&self) -> bool {
        self.live.load(Ordering::Acquire)
    }
    fn revoke(&self) {
        self.live.store(false, Ordering::Release);
        self.cancelled.send_replace(true);
    }
    pub(super) fn revoke_account_binding(&self, account: &str, connector: &str) {
        if self.account_id == account
            && self
                .binding
                .as_ref()
                .is_some_and(|b| b.connector == connector)
        {
            self.revoke();
        }
    }
}

impl AccountGrant {
    pub(crate) fn weak_state(&self) -> (String, Weak<GrantState>) {
        (self.state.id.clone(), Arc::downgrade(&self.state))
    }
    pub async fn cancelled(&self) {
        let mut cancelled = self.state.cancelled.subscribe();
        while !*cancelled.borrow_and_update() {
            if cancelled.changed().await.is_err() {
                break;
            }
        }
    }
    pub fn is_live(&self) -> bool {
        self.state.live.load(Ordering::Acquire)
    }
    pub async fn valid(&self) -> bool {
        if !self.is_live() {
            return false;
        }
        let valid = self.authority.current(self).await.unwrap_or(false);
        if !valid {
            self.state.revoke();
        }
        valid
    }
    pub(crate) async fn valid_in(&self, tx: &mut Transaction<'_, Sqlite>) -> bool {
        self.is_live() && self.authority.current_in(tx, self).await.unwrap_or(false)
    }
    pub fn same_authority(&self, other: &Self) -> bool {
        self.account_id == other.account_id
            && self.binding == other.binding
            && self.role == other.role
    }
}

impl AccountAuthority {
    pub async fn account(&self, account: &str) -> AccountResult<AccountGrant> {
        let row: Option<(Option<String>, String)> = query_as("SELECT canonical_user_id,role FROM albert_login_accounts WHERE id=? AND disabled=0 AND must_change_password=0")
            .bind(account).fetch_optional(&self.pool).await?;
        let (Some(user_id), role) = row.ok_or(AccountError::Denied)? else {
            return Err(AccountError::Denied);
        };
        let role = match role.as_str() {
            "owner" => AccountRole::Owner,
            "user" => AccountRole::User,
            _ => return Err(AccountError::Denied),
        };
        Ok(self.grant(account.into(), user_id, role, None))
    }

    pub async fn actor(
        &self,
        connector: &str,
        bot: u64,
        actor: i64,
    ) -> AccountResult<Option<AccountGrant>> {
        if self
            .bot(connector)
            .is_none_or(|ready| ready.bot_id != bot.to_string())
        {
            return Ok(None);
        }
        let row: Option<(String, String)> = query_as("SELECT account_id,generation FROM albert_telegram_bindings WHERE connector_id=? AND bot_id=? AND actor_id=? AND revoked_at IS NULL")
            .bind(connector).bind(bot.to_string()).bind(actor.to_string()).fetch_optional(&self.pool).await?;
        let Some((account, generation)) = row else {
            return Ok(None);
        };
        let mut grant = match self.account(&account).await {
            Ok(grant) => grant,
            Err(AccountError::Denied) => return Ok(None),
            Err(e) => return Err(e),
        };
        let binding = BindingScope {
            connector: connector.into(),
            bot_id: bot.to_string(),
            actor_id: actor.to_string(),
            generation,
        };
        grant = self.grant(grant.account_id, grant.user_id, grant.role, Some(binding));
        Ok(Some(grant))
    }

    fn grant(
        &self,
        account_id: String,
        user_id: String,
        role: AccountRole,
        binding: Option<BindingScope>,
    ) -> AccountGrant {
        let state = Arc::new(GrantState {
            id: SessionId::new().to_string(),
            created: Instant::now(),
            cancelled: channel(false).0,
            account_id: account_id.clone(),
            binding: binding.clone(),
            live: AtomicBool::new(true),
        });
        if let Ok(mut leases) = self.leases.lock() {
            leases.retain(|state| state.strong_count() > 0);
            leases.push(Arc::downgrade(&state));
        }
        AccountGrant {
            account_id,
            user_id,
            role,
            binding,
            authority: self.clone(),
            state,
        }
    }

    async fn current(&self, grant: &AccountGrant) -> AccountResult<bool> {
        let mut tx = self.pool.begin().await?;
        let valid = self.current_in(&mut tx, grant).await?;
        tx.commit().await?;
        Ok(valid && grant.is_live())
    }

    async fn current_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        grant: &AccountGrant,
    ) -> AccountResult<bool> {
        let role = match grant.role {
            AccountRole::Owner => "owner",
            AccountRole::User => "user",
        };
        let row: Option<(String,)> = query_as("SELECT id FROM albert_login_accounts WHERE id=? AND canonical_user_id=? AND role=? AND disabled=0 AND must_change_password=0")
            .bind(&grant.account_id).bind(&grant.user_id).bind(role).fetch_optional(&mut **tx).await?;
        if row.is_none() {
            return Ok(false);
        }
        if let Some(binding) = &grant.binding {
            let row: Option<(String,)> = query_as("SELECT generation FROM albert_telegram_bindings WHERE connector_id=? AND bot_id=? AND actor_id=? AND account_id=? AND generation=? AND revoked_at IS NULL")
                .bind(&binding.connector).bind(&binding.bot_id).bind(&binding.actor_id).bind(&grant.account_id).bind(&binding.generation).fetch_optional(&mut **tx).await?;
            return Ok(row.is_some());
        }
        Ok(true)
    }

    pub fn invalidate_account(&self, account: &str) {
        if let Ok(leases) = self.leases.lock() {
            for state in leases.iter().filter_map(|state| state.upgrade()) {
                if state.account_id == account {
                    state.revoke();
                }
            }
        }
    }
    pub fn invalidate_binding(&self, connector: &str, actor: Option<&str>) {
        if let Ok(leases) = self.leases.lock() {
            for state in leases.iter().filter_map(|state| state.upgrade()) {
                if state.binding.as_ref().is_some_and(|b| {
                    b.connector == connector && actor.is_none_or(|actor| actor == b.actor_id)
                }) {
                    state.revoke();
                }
            }
        }
    }
}
