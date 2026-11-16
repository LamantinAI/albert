use std::time::{Duration, Instant};

use serde_json::json;

use super::{native::ApprovalOwner, native::NativeRecord, ApprovalChange, ApprovalService};
use crate::api::{
    errors::ApiError, errors::ApiResult, models::ApprovalKind, models::ApprovalState,
    models::ApprovalView,
};

pub(super) struct Entry {
    pub record: NativeRecord,
    pub view: ApprovalView,
    pub in_flight: bool,
    pub uncertain: bool,
    until: Instant,
}
impl ApprovalService {
    pub(super) async fn remember(
        &self,
        owner: &ApprovalOwner,
        record: NativeRecord,
    ) -> ApiResult<ApprovalView> {
        if record.recipient_principal != owner.origin.principal {
            return Err(ApiError::forbidden());
        }
        let id = record.handle().encode();
        let session = self
            .origin_session(owner, record.origin_scope.as_ref())
            .await;
        let mut view = ApprovalView {
            id: id.clone(),
            connector: record.executor.clone(),
            kind: ApprovalKind::Native,
            session_id: session,
            title: record.title.clone(),
            action: record.action.clone(),
            effects: record.effects.clone(),
            details: record.details.clone(),
            state: record.state,
            summary: json!({"operation_hash":record.operation_hash}),
            expires_at: Some(record.expires_at),
            can_decide: record.can_decide && record.state == ApprovalState::Pending,
        };
        let mut entries = self.entries.lock().unwrap();
        entries.retain(|_, entry| !entry.view.state.terminal() || entry.until > Instant::now());
        let key = (owner.account.clone(), owner.user.clone(), id.clone());
        let previous = entries.get(&key);
        let mut in_flight = previous.is_some_and(|entry| entry.in_flight);
        let mut uncertain = previous.is_some_and(|entry| entry.uncertain);
        if view.state == ApprovalState::Unknown {
            in_flight = false;
            uncertain = true;
        }
        if let Some(previous) = previous {
            if previous.record.operation_hash != record.operation_hash {
                return Err(ApiError::conflict(
                    "The immutable operation identity changed.",
                ));
            }
            if !view.state.terminal() {
                if in_flight {
                    view.state = ApprovalState::Deciding;
                    view.can_decide = false;
                } else if uncertain {
                    view.state = ApprovalState::Unknown;
                    view.can_decide = false;
                }
            }
        } else if entries.len() >= self.runtime.sessions.settings().confirmations.max_entries {
            return Err(ApiError::unavailable(
                "Resolve an earlier protected action before opening another.",
            ));
        }
        let changed = previous.is_none_or(|entry| {
            entry.view.state != view.state
                || entry.view.can_decide != view.can_decide
                || entry.view.session_id != view.session_id
                || entry.view.title != view.title
                || entry.view.action != view.action
                || entry.view.effects != view.effects
                || entry.view.expires_at != view.expires_at
        });
        let terminal = view.state.terminal();
        let until = Instant::now()
            + Duration::from_secs(
                self.runtime
                    .sessions
                    .settings()
                    .confirmations
                    .tombstone_ttl_seconds,
            );
        entries.insert(
            key,
            Entry {
                record,
                view: view.clone(),
                in_flight: in_flight && !terminal,
                uncertain: uncertain && !terminal,
                until,
            },
        );
        drop(entries);
        if changed {
            self.notify(owner, &view);
        }
        Ok(view)
    }
    fn notify(&self, owner: &ApprovalOwner, view: &ApprovalView) {
        let _ = self.changed.send(ApprovalChange {
            account: owner.account.clone(),
            session: view.session_id.clone(),
            id: view.id.clone(),
        });
    }
    pub(super) fn cached(&self, owner: &ApprovalOwner, id: &str) -> Option<ApprovalView> {
        self.entries
            .lock()
            .unwrap()
            .get(&(owner.account.clone(), owner.user.clone(), id.into()))
            .map(|entry| entry.view.clone())
    }
    pub(super) fn begin_decision(
        &self,
        owner: &ApprovalOwner,
        id: &str,
    ) -> ApiResult<Option<NativeRecord>> {
        let mut entries = self.entries.lock().unwrap();
        let entry = entries
            .get_mut(&(owner.account.clone(), owner.user.clone(), id.into()))
            .ok_or_else(ApiError::not_found)?;
        if entry.in_flight || entry.uncertain || entry.view.state.terminal() {
            return Ok(None);
        }
        if !entry.view.can_decide {
            return Err(ApiError::conflict(
                "The action cannot currently be confirmed.",
            ));
        }
        entry.in_flight = true;
        entry.view.state = ApprovalState::Deciding;
        entry.view.can_decide = false;
        self.notify(owner, &entry.view);
        Ok(Some(entry.record.clone()))
    }
    pub(super) fn decision_uncertain(
        &self,
        owner: &ApprovalOwner,
        id: &str,
    ) -> ApiResult<ApprovalView> {
        let mut entries = self.entries.lock().unwrap();
        let entry = entries
            .get_mut(&(owner.account.clone(), owner.user.clone(), id.into()))
            .ok_or_else(ApiError::not_found)?;
        if !entry.view.state.terminal() {
            entry.in_flight = false;
            entry.uncertain = true;
            entry.view.state = ApprovalState::Unknown;
            entry.view.can_decide = false;
        }
        self.notify(owner, &entry.view);
        Ok(entry.view.clone())
    }
    pub(super) fn unresolved(&self, owner: &ApprovalOwner) -> Vec<String> {
        self.entries
            .lock()
            .unwrap()
            .iter()
            .filter(|((account, user, _), entry)| {
                account == &owner.account && user == &owner.user && !entry.view.state.terminal()
            })
            .map(|((_, _, id), _)| id.clone())
            .collect()
    }
}
