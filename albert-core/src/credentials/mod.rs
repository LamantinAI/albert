pub const CREDENTIAL_RECEIPT_MARKER: &str = "[Credential handoff receipt] ";

mod lifecycle;
mod requests;
pub mod tool;
mod types;

pub use types::{
    valid_credential_name, CredentialChange, CredentialContinuation, CredentialError,
    CredentialNotification, CredentialSettings, CredentialSource, CredentialState, CredentialView,
};

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use octo_approval::Ingress;
use octo_history::SessionKind;
use tokio::sync::broadcast::{channel, Receiver, Sender};

use crate::{
    accounts::AccountRole,
    sessions::{actor::ActorToolPolicy, SessionContext},
};

struct Entry {
    view: CredentialView,
    account: String,
    user: String,
    context: Option<SessionContext>,
    origin: Option<String>,
    ingress: Option<Ingress>,
    origin_complete: bool,
    until: Instant,
}
pub struct CredentialGuard {
    enabled: AtomicBool,
    settings: CredentialSettings,
    entries: Mutex<HashMap<String, Entry>>,
    changed: Sender<CredentialChange>,
}
impl CredentialGuard {
    pub fn new(settings: CredentialSettings) -> Arc<Self> {
        let (changed, _) = channel(128);
        Arc::new(Self {
            enabled: AtomicBool::new(false),
            settings,
            entries: Mutex::new(HashMap::new()),
            changed,
        })
    }
    pub fn enable_ui(&self) {
        self.enabled.store(true, Ordering::Release);
    }
    pub fn ui_enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }
    pub fn settings(&self) -> &CredentialSettings {
        &self.settings
    }
    pub fn subscribe(&self) -> Receiver<CredentialChange> {
        self.changed.subscribe()
    }
    fn emit(&self, entry: &Entry) {
        let _ = self.changed.send(CredentialChange {
            account_id: entry.account.clone(),
            session_id: entry.view.session_id.clone(),
            request_id: entry.view.id.clone(),
        });
    }
    fn clean(&self, entries: &mut HashMap<String, Entry>) {
        let now = Instant::now();
        for entry in entries.values_mut() {
            if entry.until <= now && entry.view.notification == CredentialNotification::Pending {
                entry.view.notification = CredentialNotification::Discarded;
                entry.until = now + Duration::from_secs(self.settings.tombstone_ttl_seconds);
                self.emit(entry);
            }
            if entry.until <= now && entry.view.state == CredentialState::Pending {
                entry.view.state = CredentialState::Expired;
                entry.until = now + Duration::from_secs(self.settings.tombstone_ttl_seconds);
                self.emit(entry);
            }
        }
        entries.retain(|_, entry| entry.until > now);
    }
    pub fn agent_allowed(context: &SessionContext) -> bool {
        context.is_private()
            && !context.chat_only()
            && context.is_live()
            && context.session().kind == SessionKind::Permanent
            && context.tool_policy() == ActorToolPolicy::ExistingHost
            && context
                .account()
                .is_some_and(|account| account.role == AccountRole::Owner)
    }

    pub fn list(&self, account: &str, user: &str, session: Option<&str>) -> Vec<CredentialView> {
        let mut entries = self.entries.lock().unwrap();
        self.clean(&mut entries);
        entries
            .values()
            .filter(|entry| {
                entry.account == account
                    && entry.user == user
                    && entry.view.session_id.as_deref() == session
                    && matches!(
                        entry.view.state,
                        CredentialState::Pending | CredentialState::Saving
                    )
            })
            .map(|entry| entry.view.clone())
            .collect()
    }
    pub fn get(
        &self,
        account: &str,
        user: &str,
        id: &str,
    ) -> Result<CredentialView, CredentialError> {
        let mut entries = self.entries.lock().unwrap();
        self.clean(&mut entries);
        let entry = entries
            .get(id)
            .filter(|entry| entry.account == account && entry.user == user)
            .ok_or(CredentialError::NotFound)?;
        Ok(entry.view.clone())
    }
    /// The caller owns the sensitive value and performs the authenticated write.
    /// The guard only invokes the closure under its lifecycle fence; it never stores a value.
    pub fn save(
        &self,
        account: &str,
        user: &str,
        id: &str,
        name: &str,
        write: impl FnOnce() -> Result<bool, ()>,
    ) -> Result<CredentialView, CredentialError> {
        if !valid_credential_name(name) {
            return Err(CredentialError::Name);
        }
        let mut entries = self.entries.lock().unwrap();
        self.clean(&mut entries);
        let entry = entries
            .get_mut(id)
            .filter(|entry| entry.account == account && entry.user == user)
            .ok_or(CredentialError::NotFound)?;
        if matches!(
            entry.view.state,
            CredentialState::Saved | CredentialState::Indeterminate
        ) {
            let mut previous = entry.view.clone();
            previous.replayed = true;
            return Ok(previous);
        }
        if entry.view.state != CredentialState::Pending {
            return Err(CredentialError::Closed);
        }
        if entry
            .context
            .as_ref()
            .is_some_and(|context| !context.is_live())
        {
            entry.view.state = CredentialState::Cancelled;
            self.emit(entry);
            return Err(CredentialError::Closed);
        }
        entry.view.state = CredentialState::Saving;
        entry.view.name = Some(name.into());
        match write() {
            Ok(replaced) => {
                entry.view.state = CredentialState::Saved;
                entry.view.replaced = replaced;
                entry.view.notification = if entry.context.is_some() {
                    CredentialNotification::Pending
                } else {
                    CredentialNotification::None
                };
            }
            Err(()) => entry.view.state = CredentialState::Indeterminate,
        }
        entry.until = Instant::now()
            + Duration::from_secs(
                if entry.view.notification == CredentialNotification::Pending {
                    self.settings.notification_ttl_seconds
                } else {
                    self.settings.tombstone_ttl_seconds
                },
            );
        self.emit(entry);
        Ok(entry.view.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::{CredentialError, CredentialGuard, CredentialSettings, CredentialState};
    use crate::selfconfig::SelfConfig;
    use dotenvy::from_path_iter;
    use octo_history::SessionId;
    use serde_json::to_string;
    use std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc, Barrier,
        },
        thread,
        time::{Duration, Instant},
    };
    use tempfile::tempdir;

    fn request(guard: &CredentialGuard) -> String {
        let id = SessionId::new().to_string();
        guard
            .create("account", "person", &id, "SUGGESTED", None, None)
            .unwrap();
        id
    }
    #[test]
    fn name_only_metadata_replays_the_first_commit_without_rewriting_a_later_value() {
        let dir = tempdir().unwrap();
        let store = SelfConfig::new(dir.path().into());
        let guard = CredentialGuard::new(CredentialSettings::default());
        let id = request(&guard);
        let count = AtomicUsize::new(0);
        let sentinel = "sensitive-fixture-value-49";
        let receipt = guard
            .save("account", "person", &id, "ACTUAL_NAME", || {
                count.fetch_add(1, Ordering::Relaxed);
                store
                    .panel_upsert_secret("ACTUAL_NAME", sentinel)
                    .map_err(|_| ())
            })
            .unwrap();
        assert_eq!(receipt.state, CredentialState::Saved);
        assert_eq!(receipt.name.as_deref(), Some("ACTUAL_NAME"));
        assert!(!to_string(&receipt).unwrap().contains(sentinel));
        store
            .panel_set_secret("ACTUAL_NAME", "newer-value")
            .unwrap();
        let replay = guard
            .save("account", "person", &id, "OTHER_NAME", || {
                count.fetch_add(1, Ordering::Relaxed);
                Ok(false)
            })
            .unwrap();
        assert_eq!(replay.name, receipt.name);
        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert_eq!(store.panel_secret_names(), vec!["ACTUAL_NAME"]);
        assert_eq!(
            from_path_iter(dir.path().join(".env"))
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .1,
            "newer-value"
        );
        assert_eq!(
            guard.get("other-account", "person", &id).unwrap_err(),
            CredentialError::NotFound
        );
    }
    #[test]
    fn cancel_and_save_share_a_single_commit_decision() {
        for _ in 0..8 {
            let guard = CredentialGuard::new(CredentialSettings::default());
            let id = request(&guard);
            let barrier = Arc::new(Barrier::new(3));
            let writes = Arc::new(AtomicUsize::new(0));
            let writer = {
                let guard = guard.clone();
                let id = id.clone();
                let barrier = barrier.clone();
                let writes = writes.clone();
                thread::spawn(move || {
                    barrier.wait();
                    guard.save("account", "person", &id, "TOKEN", || {
                        writes.fetch_add(1, Ordering::Relaxed);
                        Ok(false)
                    })
                })
            };
            let cancel = {
                let guard = guard.clone();
                let id = id.clone();
                let barrier = barrier.clone();
                thread::spawn(move || {
                    barrier.wait();
                    guard.cancel("account", "person", &id)
                })
            };
            barrier.wait();
            let _ = writer.join().unwrap();
            let _ = cancel.join().unwrap();
            let state = guard.get("account", "person", &id).unwrap().state;
            assert!(matches!(
                (state, writes.load(Ordering::Relaxed)),
                (CredentialState::Saved, 1) | (CredentialState::Cancelled, 0)
            ));
        }
    }
    #[test]
    fn expiry_bounds_and_uncertain_writes_never_blindly_reexecute() {
        let guard = CredentialGuard::new(CredentialSettings {
            max_pending: 1,
            max_entries: 2,
            ..CredentialSettings::default()
        });
        let id = request(&guard);
        assert_eq!(
            guard
                .create("other", "person", SessionId::new().as_str(), "", None, None)
                .unwrap_err(),
            CredentialError::Limit
        );
        guard.entries.lock().unwrap().get_mut(&id).unwrap().until =
            Instant::now() - Duration::from_secs(1);
        assert_eq!(
            guard.get("account", "person", &id).unwrap().state,
            CredentialState::Expired
        );
        assert_eq!(
            guard
                .save("account", "person", &id, "TOKEN", || panic!(
                    "Expired request must not write"
                ))
                .unwrap_err(),
            CredentialError::Closed
        );
        guard.entries.lock().unwrap().get_mut(&id).unwrap().until =
            Instant::now() - Duration::from_secs(1);
        assert_eq!(
            guard.get("account", "person", &id).unwrap_err(),
            CredentialError::NotFound
        );
        let id = request(&guard);
        assert_eq!(
            guard
                .save("account", "person", &id, "TOKEN", || Err(()))
                .unwrap()
                .state,
            CredentialState::Indeterminate
        );
        assert_eq!(
            guard
                .save("account", "person", &id, "TOKEN", || panic!(
                    "Unknown write must not replay"
                ))
                .unwrap()
                .state,
            CredentialState::Indeterminate
        );
    }
    #[test]
    fn separate_secret_writer_instances_preserve_concurrent_keys() {
        let dir = tempdir().unwrap();
        let barrier = Arc::new(Barrier::new(17));
        let threads: Vec<_> = (0..16)
            .map(|i| {
                let root = dir.path().to_path_buf();
                let barrier = barrier.clone();
                thread::spawn(move || {
                    barrier.wait();
                    SelfConfig::new(root)
                        .panel_set_secret(&format!("KEY_{i}"), "fixture")
                        .unwrap();
                })
            })
            .collect();
        barrier.wait();
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(
            SelfConfig::new(dir.path().into())
                .panel_secret_names()
                .len(),
            16
        );
    }
}
