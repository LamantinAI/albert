//! In-process liveness shared by every view/attempt of one Session epoch.
//! Durable markers are authoritative across restarts; the registry is hydrated
//! before connectors accept events and never grants identity or tool rights.

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, Weak,
    },
};

use octo_approval::{ScopeBinding, ScopeValidator};
use octo_history::SessionId;

use super::error::{SessionError, SessionResult};

#[derive(Debug)]
pub(crate) struct SessionLease {
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
}

#[derive(Default)]
pub(crate) struct ScopeRegistry {
    inner: Mutex<RegistryState>,
}

impl ScopeRegistry {
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
