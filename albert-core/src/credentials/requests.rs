//! Validated creation of owner-scoped credential requests.
use super::{
    valid_credential_name, CredentialError, CredentialGuard, CredentialNotification,
    CredentialSource, CredentialState, CredentialView, Entry,
};
use crate::{accounts::AccountRole, sessions::SessionContext};
use chrono::Utc;
use octo_history::SessionId;
use std::time::{Duration, Instant};
impl CredentialGuard {
    pub fn create(
        &self,
        account: &str,
        user: &str,
        id: &str,
        name: &str,
        context: Option<SessionContext>,
        origin: Option<String>,
    ) -> Result<CredentialView, CredentialError> {
        if SessionId::parse(id).is_err() {
            return Err(CredentialError::Conflict);
        }
        if origin.is_some()
            && context
                .as_ref()
                .is_none_or(|context| !Self::agent_allowed(context))
        {
            return Err(CredentialError::Denied);
        }
        if (origin.is_some() || !name.is_empty()) && !valid_credential_name(name) {
            return Err(CredentialError::Name);
        }
        if context.as_ref().is_some_and(|context| {
            !context.is_private()
                || context
                    .private_user_id()
                    .ok()
                    .is_none_or(|id| id.as_str() != user)
                || context.account().is_none_or(|grant| {
                    grant.account_id != account
                        || grant.user_id != user
                        || grant.role != AccountRole::Owner
                })
        }) {
            return Err(CredentialError::Denied);
        }
        let session = context
            .as_ref()
            .map(|context| context.session_id().to_string());
        let source = if origin.is_some() {
            CredentialSource::Agent
        } else {
            CredentialSource::Manual
        };
        let mut entries = self.entries.lock().unwrap();
        self.clean(&mut entries);
        if let Some(entry) = entries.get(id) {
            if entry.account != account
                || entry.user != user
                || entry.view.session_id != session
                || entry.view.suggested_name != name
                || entry.origin != origin
            {
                return Err(CredentialError::Conflict);
            }
            return Ok(entry.view.clone());
        }
        if origin.is_some() {
            if let Some(existing) = entries.values().find(|entry| {
                entry.account == account
                    && entry.user == user
                    && entry.view.session_id == session
                    && entry.origin == origin
                    && entry.view.suggested_name == name
            }) {
                return Ok(existing.view.clone());
            }
        }
        if let Some(existing) = entries.values().find(|entry| {
            entry.account == account
                && entry.view.session_id == session
                && matches!(
                    entry.view.state,
                    CredentialState::Pending | CredentialState::Saving
                )
        }) {
            if source == CredentialSource::Agent
                && existing.origin == origin
                && existing.view.suggested_name == name
            {
                return Ok(existing.view.clone());
            }
            return Err(CredentialError::Pending);
        }
        if entries.len() >= self.settings.max_entries
            || entries
                .values()
                .filter(|entry| entry.view.state == CredentialState::Pending)
                .count()
                >= self.settings.max_pending
        {
            return Err(CredentialError::Limit);
        }
        let view = CredentialView {
            id: id.into(),
            session_id: session,
            suggested_name: name.into(),
            name: None,
            source,
            state: CredentialState::Pending,
            notification: CredentialNotification::None,
            expires_at: Utc::now().timestamp() + self.settings.request_ttl_seconds as i64,
            replaced: false,
            replayed: false,
        };
        let entry = Entry {
            view: view.clone(),
            account: account.into(),
            user: user.into(),
            context,
            origin,
            ingress: None,
            origin_complete: source == CredentialSource::Manual,
            until: Instant::now() + Duration::from_secs(self.settings.request_ttl_seconds),
        };
        self.emit(&entry);
        entries.insert(id.into(), entry);
        Ok(view)
    }
}
