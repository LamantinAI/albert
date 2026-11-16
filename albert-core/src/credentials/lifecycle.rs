use std::time::{Duration, Instant};

use octo_approval::Ingress;

use super::{
    CredentialError, CredentialGuard, CredentialNotification, CredentialSource, CredentialState,
    CredentialView,
};
use crate::sessions::SessionContext;

impl CredentialGuard {
    pub(crate) fn attach_ingress(&self, id: &str, ingress: Option<Ingress>) {
        if let Some(entry) = self.entries.lock().unwrap().get_mut(id) {
            entry.ingress = ingress;
        }
    }
    pub fn cancel(
        &self,
        account: &str,
        user: &str,
        id: &str,
    ) -> Result<CredentialView, CredentialError> {
        let mut entries = self.entries.lock().unwrap();
        self.clean(&mut entries);
        let entry = entries
            .get_mut(id)
            .filter(|entry| entry.account == account && entry.user == user)
            .ok_or(CredentialError::NotFound)?;
        if entry.view.state == CredentialState::Pending {
            entry.view.state = CredentialState::Cancelled;
            entry.until = Instant::now() + Duration::from_secs(self.settings.tombstone_ttl_seconds);
            self.emit(entry);
        }
        Ok(entry.view.clone())
    }
    pub fn finish_origin(&self, origin: &str) {
        for entry in self
            .entries
            .lock()
            .unwrap()
            .values_mut()
            .filter(|entry| entry.origin.as_deref() == Some(origin))
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
            .filter(|entry| entry.origin.as_deref() == Some(origin) && !entry.origin_complete)
        {
            if entry.view.state == CredentialState::Pending {
                entry.view.state = CredentialState::Cancelled;
            }
            if entry.view.notification == CredentialNotification::Pending {
                entry.view.notification = CredentialNotification::Discarded;
            }
            self.emit(entry);
        }
    }
    pub fn cancel_session(&self, session: &str) {
        for entry in self
            .entries
            .lock()
            .unwrap()
            .values_mut()
            .filter(|entry| entry.view.session_id.as_deref() == Some(session))
        {
            if entry.view.state == CredentialState::Pending {
                entry.view.state = CredentialState::Cancelled;
            }
            if entry.view.notification == CredentialNotification::Pending {
                entry.view.notification = CredentialNotification::Discarded;
            }
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
            .retain(|_, entry| entry.view.session_id.as_deref() != Some(session));
    }
    pub fn deliveries(&self) -> Vec<(CredentialView, SessionContext, Option<Ingress>)> {
        let mut entries = self.entries.lock().unwrap();
        self.clean(&mut entries);
        entries
            .values()
            .filter(|entry| {
                entry.view.source == CredentialSource::Agent
                    && entry.origin_complete
                    && entry.view.state == CredentialState::Saved
                    && entry.view.notification == CredentialNotification::Pending
            })
            .filter_map(|entry| {
                Some((
                    entry.view.clone(),
                    entry.context.clone()?,
                    entry.ingress.clone(),
                ))
            })
            .collect()
    }
    pub fn manual_receipts(&self) -> Vec<(CredentialView, SessionContext)> {
        let mut entries = self.entries.lock().unwrap();
        self.clean(&mut entries);
        entries
            .values()
            .filter(|entry| {
                entry.view.source == CredentialSource::Manual
                    && entry.view.state == CredentialState::Saved
                    && entry.view.notification == CredentialNotification::Pending
            })
            .filter_map(|entry| Some((entry.view.clone(), entry.context.clone()?)))
            .collect()
    }
    pub(crate) fn notification_pending(&self, id: &str) -> bool {
        self.entries.lock().unwrap().get(id).is_some_and(|entry| {
            entry.view.state == CredentialState::Saved
                && entry.view.notification == CredentialNotification::Pending
                && entry.until > Instant::now()
        })
    }
    pub(crate) fn schedule_notification(&self, id: &str, start: impl FnOnce()) -> bool {
        let mut entries = self.entries.lock().unwrap();
        let Some(entry) = entries.get_mut(id).filter(|entry| {
            entry.view.state == CredentialState::Saved
                && entry.view.notification == CredentialNotification::Pending
                && entry.until > Instant::now()
        }) else {
            return false;
        };
        start();
        entry.view.notification = CredentialNotification::Recorded;
        entry.until = Instant::now() + Duration::from_secs(self.settings.tombstone_ttl_seconds);
        self.emit(entry);
        true
    }
    pub fn notification_recorded(&self, id: &str, delivered: bool) {
        if let Some(entry) = self.entries.lock().unwrap().get_mut(id) {
            entry.view.notification = if delivered {
                CredentialNotification::Recorded
            } else {
                CredentialNotification::Discarded
            };
            entry.until = Instant::now() + Duration::from_secs(self.settings.tombstone_ttl_seconds);
            self.emit(entry);
        }
    }
}
