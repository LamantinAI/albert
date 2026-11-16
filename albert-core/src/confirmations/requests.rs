use std::time::{Duration, Instant};

use chrono::Utc;
use octo_approval::Ingress;
use octo_history::SessionId;

use super::{
    ConfirmationError, ConfirmationGuard, ConfirmationRequest, ConfirmationState, ConfirmationView,
    Entry, Notification,
};
use crate::sessions::SessionContext;

impl ConfirmationGuard {
    pub fn create(
        &self,
        context: SessionContext,
        origin: String,
        request: ConfirmationRequest,
        ingress: Option<Ingress>,
    ) -> Result<ConfirmationView, ConfirmationError> {
        if !self.ui_enabled() || !Self::agent_allowed(&context) {
            return Err(ConfirmationError::Denied);
        }
        let settings = &self.settings;
        if origin.trim().is_empty()
            || request.title.trim().is_empty()
            || request.action.trim().is_empty()
            || request.title.len() > settings.max_title_bytes
            || request.action.len() > settings.max_action_bytes
            || request.effects.len() > settings.max_effects
            || request
                .effects
                .iter()
                .any(|value| value.len() > settings.max_effect_bytes)
            || request
                .details
                .as_ref()
                .is_some_and(|value| value.len() > settings.max_details_bytes)
        {
            return Err(ConfirmationError::Invalid);
        }
        let account = context.account().ok_or(ConfirmationError::Denied)?;
        let mut entries = self.entries.lock().unwrap();
        self.clean(&mut entries);
        if let Some(entry) = entries.values().find(|entry| {
            entry.account == account.account_id
                && entry.user == account.user_id
                && entry.view.session_id == context.session_id().as_str()
                && entry.origin == origin
                && entry.view.request == request
        }) {
            return Ok(entry.view.clone());
        }
        if entries.len() >= settings.max_entries
            || entries
                .values()
                .filter(|entry| {
                    entry.view.state == ConfirmationState::Pending
                        && !(entry.account == account.account_id
                            && entry.user == account.user_id
                            && entry.origin == origin
                            && entry.view.session_id == context.session_id().as_str())
                })
                .count()
                >= settings.max_pending
        {
            return Err(ConfirmationError::Limit);
        }
        // A corrected question in the same attempt supersedes its earlier prompt.
        // Different work origins retain their independent queue entries.
        for entry in entries.values_mut().filter(|entry| {
            entry.account == account.account_id
                && entry.user == account.user_id
                && entry.origin == origin
                && entry.view.session_id == context.session_id().as_str()
                && entry.view.state == ConfirmationState::Pending
        }) {
            entry.view.state = ConfirmationState::Superseded;
            entry.notification = Notification::Discarded;
            entry.until = Instant::now() + Duration::from_secs(settings.tombstone_ttl_seconds);
            self.emit(entry);
        }
        let view = ConfirmationView {
            id: SessionId::new().to_string(),
            session_id: context.session_id().to_string(),
            request,
            state: ConfirmationState::Pending,
            expires_at: Utc::now().timestamp() + settings.request_ttl_seconds as i64,
        };
        let entry = Entry {
            account: account.account_id.clone(),
            user: account.user_id.clone(),
            origin,
            origin_complete: false,
            context,
            ingress,
            notification: Notification::None,
            until: Instant::now() + Duration::from_secs(settings.request_ttl_seconds),
            view: view.clone(),
        };
        self.emit(&entry);
        entries.insert(view.id.clone(), entry);
        Ok(view)
    }
}
