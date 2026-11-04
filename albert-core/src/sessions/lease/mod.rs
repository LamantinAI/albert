//! In-process liveness shared by every view/attempt of one Session epoch.
//! Durable markers are authoritative across restarts; the registry is hydrated
//! before connectors accept events and never grants identity or tool rights.

use std::{
    collections::HashMap,
    sync::atomic::AtomicBool,
    sync::atomic::Ordering,
    sync::Arc,
    sync::Mutex,
    sync::Weak,
    time::{Duration, Instant},
};

use octo_approval::{ScopeBinding, ScopeValidator};
use octo_history::SessionId;

use super::{error::SessionError, error::SessionResult, SessionContext};
use crate::{accounts::GrantState, sessions::settings::SessionLimits};

#[derive(Debug)]
pub struct SessionLease {
    id: SessionId,
    epoch: Option<String>,
    live: AtomicBool,
}

impl SessionLease {
    pub fn is_live(&self) -> bool {
        self.live.load(Ordering::Acquire)
    }
    pub fn epoch(&self) -> Option<&str> {
        self.epoch.as_deref()
    }
}

#[derive(Default)]
struct RegistryState {
    leases: HashMap<String, Weak<SessionLease>>,
    scopes: HashMap<String, bool>,
    accounts: HashMap<String, (Arc<GrantState>, Arc<SessionLease>, Instant)>,
}

pub struct ScopeRegistry {
    account_ttl: Duration,
    capacity: usize,
    inner: Mutex<RegistryState>,
}

impl Default for ScopeRegistry {
    fn default() -> Self {
        Self::new(&SessionLimits::default())
    }
}

impl ScopeRegistry {
    pub fn new(limits: &SessionLimits) -> Self {
        Self {
            inner: Mutex::default(),
            account_ttl: Duration::from_secs(limits.account_scope_ttl_seconds),
            capacity: limits.max_account_scopes,
        }
    }

    /// Bind pending effects to this exact native account proof and Session lease.
    /// A logout invalidates only proofs issued for its Telegram binding.
    pub fn binding(&self, context: &SessionContext) -> SessionResult<Option<ScopeBinding>> {
        if let Some(account) = context.account() {
            let (id, grant) = account.weak_state();
            let key = format!("account/{id}");
            let epoch = context.session_id().to_string();
            let mut state = self.inner.lock().map_err(|_| SessionError::Unavailable)?;
            if !context.is_live() {
                return Err(SessionError::Denied);
            }
            let now = Instant::now();
            let grant = grant.upgrade().ok_or(SessionError::Denied)?;
            let deadline = grant.created + self.account_ttl;
            if deadline <= now {
                return Err(SessionError::Denied);
            }
            state.accounts.retain(|_, (grant, lease, expires)| {
                *expires > now && grant.is_live() && lease.is_live()
            });
            let scope_key = format!("{key}/{epoch}");
            if !state.accounts.contains_key(&scope_key) {
                if state.accounts.len() >= self.capacity {
                    return Err(SessionError::Unavailable);
                }
                state
                    .accounts
                    .insert(scope_key, (grant, context.lease.clone(), deadline));
            }
            return ScopeBinding::new(key, epoch)
                .map(Some)
                .map_err(|_| SessionError::Denied);
        }
        context
            .scope_epoch()
            .map(|epoch| {
                ScopeBinding::new(context.root_history_key(), epoch)
                    .map_err(|_| SessionError::Denied)
            })
            .transpose()
    }

    /// Call only after checking the current durable marker. A closed scope is
    /// never reopened merely because the final old Arc was dropped.
    pub fn issue(&self, id: &SessionId, epoch: Option<&str>) -> SessionResult<Arc<SessionLease>> {
        let key = match epoch {
            Some(epoch) => format!("session/{id}/{epoch}"),
            None => format!("session/{id}"),
        };
        let mut state = self.inner.lock().map_err(|_| SessionError::Unavailable)?;
        if let Some(lease) = state.leases.get(&key).and_then(Weak::upgrade) {
            return Ok(lease);
        }
        let live = if epoch.is_some() {
            *state.scopes.entry(key.clone()).or_insert(true)
        } else {
            true
        };
        let lease = Arc::new(SessionLease {
            id: id.clone(),
            epoch: epoch.map(str::to_owned),
            live: AtomicBool::new(live),
        });
        state.leases.retain(|_, lease| lease.strong_count() != 0);
        state.leases.insert(key, Arc::downgrade(&lease));
        Ok(lease)
    }

    pub fn restore(&self, id: &SessionId, epoch: &str, live: bool) -> SessionResult<()> {
        let mut state = self.inner.lock().map_err(|_| SessionError::Unavailable)?;
        state.scopes.insert(format!("session/{id}/{epoch}"), live);
        Ok(())
    }

    pub fn close(&self, id: &SessionId) -> SessionResult<()> {
        let mut state = self.inner.lock().map_err(|_| SessionError::Unavailable)?;
        for lease in state.leases.values().filter_map(Weak::upgrade) {
            if lease.id == *id {
                lease.live.store(false, Ordering::Release);
            }
        }
        let prefix = format!("session/{id}/");
        for (binding, live) in &mut state.scopes {
            if binding.starts_with(&prefix) {
                *live = false;
            }
        }
        Ok(())
    }

    pub fn allows(&self, binding: &str) -> bool {
        self.inner
            .lock()
            .ok()
            .and_then(|state| state.scopes.get(binding).copied())
            == Some(true)
    }
}

impl ScopeValidator for ScopeRegistry {
    fn allows(&self, binding: &ScopeBinding) -> bool {
        if binding.opaque_id.starts_with("account/") {
            return self.inner.lock().ok().and_then(|state| {
                let (grant, lease, expires) = state
                    .accounts
                    .get(&format!("{}/{}", binding.opaque_id, binding.epoch))?;
                Some(*expires > Instant::now() && grant.is_live() && lease.is_live())
            }) == Some(true);
        }
        let Some(id) = binding.opaque_id.strip_prefix("session/") else {
            return false;
        };
        if SessionId::parse(id).is_err() || binding.epoch.is_empty() || binding.epoch.contains('/')
        {
            return false;
        }
        self.allows(&format!("{}/{}", binding.opaque_id, binding.epoch))
    }
}

#[cfg(test)]
mod tests {
    use octo_approval::{ScopeBinding, ScopeValidator};
    use octo_history::SessionId;

    use super::ScopeRegistry;

    #[test]
    fn restored_saved_scope_is_active_but_old_temporary_and_unknown_scopes_are_denied() {
        let saved = SessionId::new();
        let temporary = SessionId::new();
        let registry = ScopeRegistry::default();
        registry.restore(&saved, "original-epoch", true).unwrap();
        registry.restore(&temporary, "old-runtime", false).unwrap();
        let saved_binding =
            ScopeBinding::new(format!("session/{saved}"), "original-epoch").unwrap();
        assert!(ScopeValidator::allows(&registry, &saved_binding));
        assert!(!ScopeValidator::allows(
            &registry,
            &ScopeBinding::new(format!("session/{temporary}"), "old-runtime").unwrap()
        ));
        assert!(!ScopeValidator::allows(
            &registry,
            &ScopeBinding::new(format!("session/{saved}"), "invented").unwrap()
        ));
        registry.close(&saved).unwrap();
        assert!(!ScopeValidator::allows(&registry, &saved_binding));
        assert!(!registry
            .issue(&saved, Some("original-epoch"))
            .unwrap()
            .is_live());
    }
}
