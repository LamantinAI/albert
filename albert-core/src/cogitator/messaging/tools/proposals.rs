use super::{SendProposal, SessionMessagingTool};
use crate::cogitator::messaging::routes::RuntimeRoutes;
use crate::{
    cogitator::AlbertCogitator, messaging::contract::MessageDraft, messaging::contract::MessageId,
    messaging::contract::MessageIntent, messaging::contract::WorkReportPolicy,
    messaging::MessagingError, messaging::MessagingResult,
};
use chrono::Utc;
use octo_history::SessionKind;
use serde_json::{json, Value};
impl SessionMessagingTool {
    pub(in crate::cogitator::messaging) async fn admit(
        &self,
        host: &AlbertCogitator,
        proposal: &MessageDraft,
        policy: Option<&WorkReportPolicy>,
        guard: &RuntimeRoutes,
    ) -> MessagingResult<Value> {
        let service = host
            .session_service
            .as_ref()
            .ok_or(MessagingError::Unavailable)?;
        service
            .messaging_preflight(self.source.context(), &proposal.destination)
            .await?;
        if let Some(policy) = policy {
            for target in [&policy.intermediate, &policy.final_result]
                .into_iter()
                .flatten()
            {
                service
                    .messaging_preflight(self.source.context(), target)
                    .await?;
            }
        }
        for context in &proposal.context {
            let session = service
                .get(
                    self.source.context(),
                    context.reference.journal.session_id(),
                )
                .await?;
            if session.kind == SessionKind::Temporary {
                return Err(MessagingError::SaveRequired);
            }
        }
        let receipt = self
            .runtime
            .store
            .send_with_policy(
                &self.source,
                proposal,
                policy,
                guard,
                Utc::now().timestamp_millis(),
            )
            .await?;
        let observation = self.observe_admitted_recipient(host, proposal).await;
        Ok(
            json!({"recipient_observation":observation,"queue_note":if proposal.intent.schedules_work() {"Accepted asynchronously, not in parallel with active recipient work. It waits for an idle Session boundary under scheduler limits. For already available state use passive inspect/status; an explicit request to ask the recipient remains queued."} else {"Passive notice; no model turn is scheduled."},"message_id":proposal.id,"accepted":true,"duplicate":receipt.duplicate,"source_record":receipt.source,"physical_delivery":"pending_dispatch","work":if proposal.intent.schedules_work() {"accepted_for_async_processing"} else {"passive"},"report_policy_bound":policy.is_some()}),
        )
    }
    pub(in crate::cogitator::messaging) fn prepare(
        &self,
        message: SendProposal,
    ) -> MessagingResult<MessageDraft> {
        if message.intent == MessageIntent::Response {
            return Err(MessagingError::Invalid(
                "response identity is supplied by the host",
            ));
        }
        let seconds = message
            .timeout_secs
            .unwrap_or(self.runtime.settings.max_work_secs);
        if seconds == 0 || seconds > self.runtime.settings.max_work_secs {
            return Err(MessagingError::Invalid("timeout exceeds messaging limit"));
        }
        let ceiling = self.clamp(message.intent, message.ceiling)?;
        Ok(MessageDraft {
            id: MessageId::new(),
            destination: message.destination,
            intent: message.intent,
            content: message.content,
            correlation: None,
            causation: None,
            deadline_unix_ms: Utc::now().timestamp_millis() + seconds as i64 * 1000,
            hops_remaining: self.runtime.settings.limits.max_hops,
            context: message.context,
            ceiling,
        })
    }
}
