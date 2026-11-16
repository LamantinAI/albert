//! Bounded conversational human decisions. These never authorize native effects.
pub const CONFIRMATION_RECEIPT_MARKER: &str = "[Human confirmation receipt] ";
mod lifecycle;
mod requests;
mod settings;
pub mod tool;
mod types;

pub use self::{
    settings::ConfirmationSettings, types::ConfirmationChange, types::ConfirmationContinuation,
    types::ConfirmationError, types::ConfirmationRequest, types::ConfirmationState,
    types::ConfirmationView, types::ConfirmationWait,
};

use std::{
    collections::HashMap, sync::atomic::AtomicBool, sync::atomic::Ordering, sync::Arc, sync::Mutex,
    time::Duration, time::Instant,
};

use octo_approval::Ingress;
use octo_history::SessionKind;
use tokio::sync::broadcast::{channel, Receiver, Sender};

use self::types::Notification;
use crate::{accounts::AccountRole, sessions::actor::ActorToolPolicy, sessions::SessionContext};

struct Entry {
    view: ConfirmationView,
    account: String,
    user: String,
    origin: String,
    origin_complete: bool,
    context: SessionContext,
    ingress: Option<Ingress>,
    notification: Notification,
    until: Instant,
}
pub struct ConfirmationGuard {
    enabled: AtomicBool,
    settings: ConfirmationSettings,
    entries: Mutex<HashMap<String, Entry>>,
    changed: Sender<ConfirmationChange>,
}
impl ConfirmationGuard {
    pub fn new(settings: ConfirmationSettings) -> Arc<Self> {
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
    pub fn settings(&self) -> &ConfirmationSettings {
        &self.settings
    }
    pub fn subscribe(&self) -> Receiver<ConfirmationChange> {
        self.changed.subscribe()
    }
    pub fn agent_allowed(context: &SessionContext) -> bool {
        context.session().kind == SessionKind::Permanent
            && context.is_live()
            && context.is_private()
            && context.tool_policy() == ActorToolPolicy::ExistingHost
            && context
                .account()
                .is_some_and(|account| account.role == AccountRole::Owner)
    }
    fn emit(&self, entry: &Entry) {
        let _ = self.changed.send(ConfirmationChange {
            account_id: entry.account.clone(),
            session_id: entry.view.session_id.clone(),
            request_id: entry.view.id.clone(),
        });
    }
    fn clean(&self, entries: &mut HashMap<String, Entry>) {
        let now = Instant::now();
        entries.retain(|_, entry| {
            if entry.until > now {
                return true;
            }
            if entry.view.state == ConfirmationState::Pending {
                entry.view.state = ConfirmationState::Expired;
                entry.notification = Notification::Discarded;
                entry.until = now + Duration::from_secs(self.settings.tombstone_ttl_seconds);
                self.emit(entry);
                return true;
            }
            false
        });
    }
    pub fn list(&self, account: &str, user: &str, session: Option<&str>) -> Vec<ConfirmationView> {
        let mut entries = self.entries.lock().unwrap();
        self.clean(&mut entries);
        let mut views: Vec<_> = entries
            .values()
            .filter(|entry| {
                entry.account == account
                    && entry.user == user
                    && session.is_none_or(|id| entry.view.session_id == id)
            })
            .map(|entry| entry.view.clone())
            .collect();
        views.sort_by(|a, b| a.id.cmp(&b.id));
        views
    }
    pub fn get(
        &self,
        account: &str,
        user: &str,
        id: &str,
    ) -> Result<ConfirmationView, ConfirmationError> {
        let mut entries = self.entries.lock().unwrap();
        self.clean(&mut entries);
        entries
            .get(id)
            .filter(|entry| entry.account == account && entry.user == user)
            .map(|entry| entry.view.clone())
            .ok_or(ConfirmationError::NotFound)
    }
    pub async fn decide(
        &self,
        account: &str,
        user: &str,
        id: &str,
        approve: bool,
    ) -> Result<ConfirmationView, ConfirmationError> {
        let context = {
            let entries = self.entries.lock().unwrap();
            entries
                .get(id)
                .filter(|entry| entry.account == account && entry.user == user)
                .map(|entry| entry.context.clone())
                .ok_or(ConfirmationError::NotFound)?
        };
        if !Self::agent_allowed(&context) || !context.authorized().await {
            return Err(ConfirmationError::Denied);
        }
        let mut entries = self.entries.lock().unwrap();
        self.clean(&mut entries);
        let entry = entries.get_mut(id).ok_or(ConfirmationError::NotFound)?;
        let outcome = if approve {
            ConfirmationState::Confirmed
        } else {
            ConfirmationState::Denied
        };
        if entry.view.state == outcome {
            return Ok(entry.view.clone());
        }
        if entry.view.state != ConfirmationState::Pending {
            return Err(ConfirmationError::Conflict);
        }
        entry.view.state = outcome;
        entry.notification = Notification::Pending;
        entry.until = Instant::now() + Duration::from_secs(self.settings.notification_ttl_seconds);
        self.emit(entry);
        Ok(entry.view.clone())
    }
    pub fn origin_waiting(&self, origin: &str) -> bool {
        let mut entries = self.entries.lock().unwrap();
        self.clean(&mut entries);
        entries
            .values()
            .any(|entry| entry.origin == origin && entry.view.state == ConfirmationState::Pending)
    }
}

#[cfg(test)]
mod tests {
    use super::{ConfirmationGuard, ConfirmationRequest, ConfirmationSettings, ConfirmationState};
    use crate::{
        db::repo::identity::bind, service::session::web::WebActor,
        service::session::SessionService, sessions::actor::ActorToolPolicy,
        sessions::settings::SessionSettings, sessions::SessionContext, sessions::UserId,
    };
    use octo_history::SqliteHistory;
    use sqlx::query;
    use std::{
        sync::atomic::AtomicUsize, sync::atomic::Ordering, sync::Arc, time::Duration, time::Instant,
    };
    use tempfile::{tempdir, TempDir};

    async fn fixture() -> (TempDir, SessionService, SessionContext) {
        let directory = tempdir().unwrap();
        let history = Arc::new(
            SqliteHistory::open_retained(directory.path().join("history.db"))
                .await
                .unwrap(),
        );
        let service = SessionService::from_history(history, SessionSettings::default())
            .await
            .unwrap();
        let mut tx = service.history().pool().begin().await.unwrap();
        bind(&mut tx, "web", "account", "person").await.unwrap();
        tx.commit().await.unwrap();
        query("INSERT INTO albert_login_accounts(id,username,password_hash,role,canonical_user_id,must_change_password) VALUES ('account','fixture','unused','owner','person',0)").execute(service.database().pool()).await.unwrap();
        let actor = WebActor {
            account_id: "account".into(),
            user_id: UserId::parse("person").unwrap(),
            tools: ActorToolPolicy::ExistingHost,
        };
        let session = service
            .web_create(&actor, "Confirmation fixture", false)
            .await
            .unwrap();
        let context = service
            .web_context(&actor, &session.id)
            .await
            .unwrap()
            .with_account(service.accounts().account("account").await.unwrap());
        service.confirmations().enable_ui();
        (directory, service, context)
    }
    fn question(action: &str) -> ConfirmationRequest {
        ConfirmationRequest {
            title: "Continue the task?".into(),
            action: action.into(),
            effects: vec!["The next phase can begin".into()],
            details: None,
        }
    }
    #[tokio::test]
    async fn confirmation_and_denial_each_continue_once_only_after_origin_finishes() {
        for approve in [true, false] {
            let (_directory, service, context) = fixture().await;
            let guard = service.confirmations();
            let view = guard
                .create(context, "origin".into(), question("Review the draft"), None)
                .unwrap();
            let decided = guard
                .decide("account", "person", &view.id, approve)
                .await
                .unwrap();
            assert_eq!(
                decided.state,
                if approve {
                    ConfirmationState::Confirmed
                } else {
                    ConfirmationState::Denied
                }
            );
            assert_eq!(
                guard
                    .decide("account", "person", &view.id, approve)
                    .await
                    .unwrap()
                    .state,
                decided.state
            );
            assert!(guard
                .decide("account", "person", &view.id, !approve)
                .await
                .is_err());
            assert!(guard.deliveries().is_empty());
            guard.finish_origin("origin");
            assert_eq!(guard.deliveries().len(), 1);
            let starts = AtomicUsize::new(0);
            assert!(guard.schedule_notification(&view.id, || {
                starts.fetch_add(1, Ordering::SeqCst);
            }));
            assert!(!guard.schedule_notification(&view.id, || {
                starts.fetch_add(1, Ordering::SeqCst);
            }));
            assert_eq!(starts.load(Ordering::SeqCst), 1);
            assert!(guard.deliveries().is_empty());
        }
    }
    #[tokio::test]
    async fn retry_deduplicates_and_correction_supersedes_without_crossing_work_origins() {
        let (_directory, service, context) = fixture().await;
        let guard = service.confirmations();
        let first = guard
            .create(
                context.clone(),
                "first".into(),
                question("Initial action"),
                None,
            )
            .unwrap();
        assert_eq!(
            guard
                .create(
                    context.clone(),
                    "first".into(),
                    question("Initial action"),
                    None
                )
                .unwrap()
                .id,
            first.id
        );
        let other = guard
            .create(
                context.clone(),
                "second".into(),
                question("Other work"),
                None,
            )
            .unwrap();
        let corrected = guard
            .create(context, "first".into(), question("Corrected action"), None)
            .unwrap();
        assert_ne!(corrected.id, first.id);
        assert_eq!(
            guard.get("account", "person", &first.id).unwrap().state,
            ConfirmationState::Superseded
        );
        assert_eq!(
            guard.get("account", "person", &other.id).unwrap().state,
            ConfirmationState::Pending
        );
        assert!(guard.get("foreign", "person", &corrected.id).is_err());
    }
    #[tokio::test]
    async fn expiration_cancellation_revocation_and_oversized_questions_never_resume() {
        let (_directory, service, context) = fixture().await;
        let guard = service.confirmations();
        let first = guard
            .create(context.clone(), "first".into(), question("Expire"), None)
            .unwrap();
        guard
            .entries
            .lock()
            .unwrap()
            .get_mut(&first.id)
            .unwrap()
            .until = Instant::now() - Duration::from_secs(1);
        assert_eq!(
            guard.get("account", "person", &first.id).unwrap().state,
            ConfirmationState::Expired
        );
        assert!(guard
            .decide("account", "person", &first.id, true)
            .await
            .is_err());
        let second = guard
            .create(context.clone(), "second".into(), question("Cancel"), None)
            .unwrap();
        guard.cancel_origin("second");
        guard.finish_origin("second");
        assert_eq!(
            guard.get("account", "person", &second.id).unwrap().state,
            ConfirmationState::Cancelled
        );
        assert!(guard.deliveries().is_empty());
        let third = guard
            .create(context.clone(), "third".into(), question("Revoke"), None)
            .unwrap();
        query("UPDATE albert_login_accounts SET role='user' WHERE id='account'")
            .execute(service.database().pool())
            .await
            .unwrap();
        assert!(guard
            .decide("account", "person", &third.id, true)
            .await
            .is_err());
        assert_eq!(
            guard.get("account", "person", &third.id).unwrap().state,
            ConfirmationState::Pending
        );
        let bounded = ConfirmationGuard::new(ConfirmationSettings {
            max_action_bytes: 4,
            ..Default::default()
        });
        bounded.enable_ui();
        assert!(bounded
            .create(context, "big".into(), question("Too large"), None)
            .is_err());
    }
    #[tokio::test]
    async fn correcting_one_pending_question_does_not_consume_a_second_pending_slot() {
        let (_directory, _service, context) = fixture().await;
        let guard = ConfirmationGuard::new(ConfirmationSettings {
            max_pending: 1,
            ..Default::default()
        });
        guard.enable_ui();
        guard
            .create(
                context.clone(),
                "origin".into(),
                question("First wording"),
                None,
            )
            .unwrap();
        let corrected = guard
            .create(
                context.clone(),
                "origin".into(),
                question("Corrected wording"),
                None,
            )
            .unwrap();
        assert_eq!(corrected.state, ConfirmationState::Pending);
        assert!(guard
            .create(
                context,
                "another-origin".into(),
                question("Independent request"),
                None
            )
            .is_err());
    }
}
