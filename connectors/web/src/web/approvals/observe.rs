use albert_core::accounts::AccountRole;
use octo_core::Envelope;

use super::ApprovalService;

impl ApprovalService {
    pub async fn observe(&self, event: &Envelope) {
        for adapter in &self.adapters {
            let Some(record) = adapter.observe(event) else {
                continue;
            };
            let Some(account) = record.origin_scope.as_ref().and_then(|scope| {
                self.runtime
                    .sessions
                    .scope_registry()
                    .account_for_scope(scope)
            }) else {
                continue;
            };
            let Ok(grant) = self.runtime.sessions.accounts().account(&account).await else {
                continue;
            };
            if grant.role != AccountRole::Owner {
                continue;
            }
            let Ok(owner) = self.owner_identity(&grant.account_id, &grant.user_id).await else {
                continue;
            };
            if self
                .origin_session(&owner, record.origin_scope.as_ref())
                .await
                .is_none()
            {
                continue;
            }
            let _ = self.remember(&owner, record).await;
        }
    }
}
