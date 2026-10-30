use serde_json::{json, Value};

use super::SessionMessagingTool;
use crate::{
    cogitator::AlbertCogitator,
    messaging::{
        contract::{MessageId, ReturnAddress, TrustedSource},
        store::{DeliveryFailure, WorkState, WorkTerminal},
        MessagingError, MessagingResult,
    },
    sessions::SessionContext,
};

impl SessionMessagingTool {
    pub(super) async fn cancel(
        &self,
        host: &AlbertCogitator,
        id: &MessageId,
    ) -> MessagingResult<Value> {
        let status = self.runtime.store.status(&self.source, id).await?;
        if status
            .work
            .as_ref()
            .is_some_and(|state| !matches!(state, WorkState::Queued | WorkState::Running))
        {
            return Err(MessagingError::Conflict);
        }
        if let Some(target) = &status.resolved {
            let key = ("session".into(), format!("session/{}", target.session));
            let state = host.turns.lock().unwrap().get(&key).cloned();
            let mut stopped = false;
            if let Some(state) = state {
                let mut gate = state.lock().await;
                if gate.message.as_ref().is_some_and(|task| &task.id == id) {
                    if let Some(task) = gate.message.take() {
                        task.stop(host, &self.ctx, WorkTerminal::Cancelled).await;
                        stopped = true;
                    }
                }
            }
            if !stopped && matches!(status.work, Some(WorkState::Queued)) {
                let service = host
                    .session_service
                    .as_ref()
                    .ok_or(MessagingError::Unavailable)?;
                let address = ReturnAddress {
                    session: status.intended.source.clone(),
                    display: status.intended.return_address.display.clone(),
                };
                let context = service
                    .messaging_context(&status.intended.actor, &target.session, &address)
                    .await?;
                let source = self.source_for_context(context)?;
                self.runtime
                    .store
                    .finish_work(&source, id, WorkTerminal::Cancelled)
                    .await?;
            }
        }
        // Only pending/retry physical work is cancelled here. Started/unknown
        // attempts require the S3 adapter callback; never claim they were unsent.
        let delivery_cancelled = self
            .runtime
            .store
            .fail_delivery(id, DeliveryFailure::Cancelled)
            .await
            .is_ok();
        Ok(
            json!({"message_id":id,"work":"cancelled","pending_delivery_cancelled":delivery_cancelled,"physical_outcome":"consult_status"}),
        )
    }
    fn source_for_context(&self, context: SessionContext) -> MessagingResult<TrustedSource> {
        TrustedSource::current(
            &context,
            self.source.work().clone(),
            self.source.execution().clone(),
        )
    }
}
