use std::time::{Duration, Instant};

use octo_approval::Ingress;

use super::{ConfirmationGuard, ConfirmationState, ConfirmationView, Notification};
use crate::sessions::SessionContext;

impl ConfirmationGuard {
    pub fn finish_origin(&self, origin: &str) {
        for entry in self
            .entries
            .lock()
            .unwrap()
            .values_mut()
            .filter(|entry| entry.origin == origin)
        {
            entry.origin_complete = true;
        }
    }
    pub fn cancel_origin(&self, origin: &str) {
        for entry in self
            .entries
            .lock()
            .unwrap()
            .values_mut()
            .filter(|entry| entry.origin == origin && !entry.origin_complete)
        {
            if entry.view.state == ConfirmationState::Pending {
                entry.view.state = ConfirmationState::Cancelled;
            }
            if entry.notification == Notification::Pending {
                entry.notification = Notification::Discarded;
            }
            entry.until = Instant::now() + Duration::from_secs(self.settings.tombstone_ttl_seconds);
            self.emit(entry);
        }
    }
    pub fn cancel_session(&self, session: &str) {
        for entry in self
            .entries
            .lock()
            .unwrap()
            .values_mut()
            .filter(|entry| entry.view.session_id == session)
        {
            if entry.view.state == ConfirmationState::Pending {
                entry.view.state = ConfirmationState::Cancelled;
            }
            if entry.notification == Notification::Pending {
                entry.notification = Notification::Discarded;
            }
            entry.until = Instant::now() + Duration::from_secs(self.settings.tombstone_ttl_seconds);
            self.emit(entry);
        }
    }
    /// Forget request presentation only after the host has fenced and stopped
    /// the conversation. Saved credentials and completed external effects remain.
    pub fn forget_session(&self, session: &str) {
        self.cancel_session(session);
        self.entries
            .lock()
            .unwrap()
            .retain(|_, entry| entry.view.session_id != session);
    }
    pub fn deliveries(&self) -> Vec<(ConfirmationView, SessionContext, Option<Ingress>)> {
        let mut entries = self.entries.lock().unwrap();
        self.clean(&mut entries);
        entries
            .values()
            .filter(|entry| {
                entry.origin_complete
                    && entry.notification == Notification::Pending
                    && matches!(
                        entry.view.state,
                        ConfirmationState::Confirmed | ConfirmationState::Denied
                    )
            })
            .map(|entry| {
                (
                    entry.view.clone(),
                    entry.context.clone(),
                    entry.ingress.clone(),
                )
            })
            .collect()
    }
    pub(crate) fn notification_pending(&self, id: &str) -> bool {
        self.entries.lock().unwrap().get(id).is_some_and(|entry| {
            entry.notification == Notification::Pending
                && matches!(
                    entry.view.state,
                    ConfirmationState::Confirmed | ConfirmationState::Denied
                )
                && entry.until > Instant::now()
        })
    }
    pub(crate) fn schedule_notification(&self, id: &str, start: impl FnOnce()) -> bool {
        let mut entries = self.entries.lock().unwrap();
        let Some(entry) = entries.get_mut(id).filter(|entry| {
            entry.notification == Notification::Pending
                && matches!(
                    entry.view.state,
                    ConfirmationState::Confirmed | ConfirmationState::Denied
                )
                && entry.until > Instant::now()
        }) else {
            return false;
        };
        start();
        entry.notification = Notification::Recorded;
        entry.until = Instant::now() + Duration::from_secs(self.settings.tombstone_ttl_seconds);
        self.emit(entry);
        true
    }
    pub fn discard_notification(&self, id: &str) {
        if let Some(entry) = self.entries.lock().unwrap().get_mut(id) {
            entry.notification = Notification::Discarded;
            entry.until = Instant::now() + Duration::from_secs(self.settings.tombstone_ttl_seconds);
            self.emit(entry);
        }
    }
}
