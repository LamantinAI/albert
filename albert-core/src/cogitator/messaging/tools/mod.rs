mod contract;
mod native;
mod proposals;
pub(in crate::cogitator) use self::contract::{MessagingAction, SendProposal};
pub(in crate::cogitator::messaging) mod authority;
mod busy;
mod control;
mod inspection;

use std::{sync::Arc, sync::Weak};

use chrono::Utc;
use octo_core::CogitatorContext;
use octo_history::{AppendRequest, SessionKind};
use serde_json::{json, to_value, Value};

use self::inspection::InspectionRequest;
use super::{routes::RuntimeRoutes, MessagingRuntime};
use crate::{
    cogitator::AlbertCogitator, history::assistant_turn, messaging::contract::DestinationSpec,
    messaging::contract::ExecutionLocation, messaging::contract::MessageIntent,
    messaging::contract::ReportClass, messaging::contract::TrustedSource,
    messaging::contract::WorkRef, messaging::MessagingError, messaging::MessagingResult,
    sessions::SessionContext,
};

#[derive(Clone)]
pub(in crate::cogitator) struct SessionMessagingTool {
    host: Weak<AlbertCogitator>,
    ctx: CogitatorContext,
    runtime: Arc<MessagingRuntime>,
    source: TrustedSource,
}

impl SessionMessagingTool {
    pub fn current(
        host: &Arc<AlbertCogitator>,
        ctx: &CogitatorContext,
        session: &SessionContext,
        work: WorkRef,
    ) -> Option<Self> {
        if !session.is_private() {
            return None;
        }
        let runtime = host.messaging.clone()?;
        let source = TrustedSource::current(session, work, ExecutionLocation::default()).ok()?;
        Some(Self {
            host: Arc::downgrade(host),
            ctx: ctx.clone(),
            runtime,
            source,
        })
    }
    async fn invoke(&self, action: MessagingAction) -> MessagingResult<Value> {
        let host = self.host.upgrade().ok_or(MessagingError::Unavailable)?;
        let service = host
            .session_service
            .as_ref()
            .ok_or(MessagingError::Unavailable)?;
        service.validate_context(self.source.context()).await?;
        if self.source.context().session().kind == SessionKind::Temporary {
            return Err(MessagingError::SaveRequired);
        }
        let guard = RuntimeRoutes::with_registry(&self.ctx, self.runtime.adapters.clone());
        let store = &self.runtime.store;
        match action {
            MessagingAction::Discover {
                limit,
                before_id,
                through_id,
                route_offset,
            } => {
                self.discover_bounded(
                    service,
                    &guard,
                    limit.unwrap_or(10),
                    before_id,
                    through_id,
                    route_offset.unwrap_or(0),
                )
                .await
            }
            MessagingAction::Inspect {
                session_id,
                limit,
                before_id,
                through_id,
                record_id,
                text_offset,
            } => {
                self.inspect_bounded(
                    service,
                    InspectionRequest {
                        session_id,
                        limit: limit.unwrap_or(10),
                        before: before_id,
                        through: through_id,
                        record_id,
                        offset: text_offset.unwrap_or(0),
                    },
                )
                .await
            }
            MessagingAction::Send { message } => {
                let policy = message.policy.clone();
                let proposal = self.prepare(message)?;
                self.admit(&host, &proposal, policy.as_ref(), &guard).await
            }
            MessagingAction::Status { message_id } => {
                let mut status = to_value(store.status(&self.source, &message_id).await?)?;
                status["observed_at"] = Utc::now().to_rfc3339().into();
                Ok(status)
            }
            MessagingAction::Cancel { message_id } => self.cancel(&host, &message_id).await,
            MessagingAction::Steer {
                message_id,
                content,
            } => {
                let previous = store.status(&self.source, &message_id).await?;
                let destination = previous
                    .resolved
                    .map(|target| DestinationSpec::FixedSession {
                        session: target.session,
                        display: target.display,
                    })
                    .unwrap_or(previous.intended.intended);
                let mut proposal = self.prepare(SendProposal {
                    destination,
                    intent: previous.intended.intent,
                    content,
                    timeout_secs: None,
                    ceiling: previous.intended.ceiling,
                    policy: None,
                    context: vec![],
                })?;
                proposal.causation = Some(message_id.clone());
                proposal.correlation = Some(message_id.clone());
                let policy = store
                    .message_report_policy(&self.source, &message_id)
                    .await?
                    .map(|mut policy| {
                        policy.revision = 1;
                        policy
                    });
                let cancelled = self.cancel(&host, &message_id).await?;
                match self.admit(&host, &proposal, policy.as_ref(), &guard).await {
                    Ok(replacement) => Ok(json!({"previous":cancelled,"replacement":replacement})),
                    Err(error) => Ok(
                        json!({"previous":cancelled,"replacement":{"status":"not_accepted","error":error.to_string()}}),
                    ),
                }
            }
            MessagingAction::Report { content } => {
                let policy = store
                    .report_policy(&self.source)
                    .await?
                    .ok_or(MessagingError::Invalid("no report policy for current work"))?;
                let destination = policy.intermediate.ok_or(MessagingError::Invalid(
                    "no intermediate report destination",
                ))?;
                let proposal = self.prepare(SendProposal {
                    destination,
                    intent: MessageIntent::Notice,
                    content: content.clone(),
                    timeout_secs: None,
                    ceiling: None,
                    policy: None,
                    context: vec![],
                })?;
                let request = AppendRequest {
                    namespace: "albert.explicit-progress.v1".into(),
                    idempotency_key: proposal.id.as_str().into(),
                    digest: proposal.id.as_str().into(),
                    turns: vec![assistant_turn(content)],
                };
                let receipt = store
                    .commit_report(
                        &self.source,
                        &request,
                        ReportClass::Intermediate,
                        Some(&proposal),
                        &guard,
                        Utc::now().timestamp_millis(),
                    )
                    .await?;
                Ok(
                    json!({"message_id":proposal.id,"canonical_report":receipt.result,"physical_delivery":"consult_message_status","report_outcome":receipt.report}),
                )
            }
            MessagingAction::Reports { message_id, policy } => {
                if let Some(policy) = policy {
                    for target in [&policy.intermediate, &policy.final_result]
                        .into_iter()
                        .flatten()
                    {
                        service
                            .messaging_preflight(self.source.context(), target)
                            .await?;
                    }
                    if let Some(id) = &message_id {
                        store
                            .set_message_report_policy(&self.source, id, &policy, &guard)
                            .await?;
                    } else {
                        store
                            .set_report_policy(&self.source, &policy, &guard)
                            .await?;
                    }
                }
                let policy = if let Some(id) = &message_id {
                    store.message_report_policy(&self.source, id).await?
                } else {
                    store.report_policy(&self.source).await?
                };
                Ok(
                    json!({"work":self.source.work(),"message_id":message_id,"policy":policy,"note":"message_id targets that admitted request's eventual reports. Its pinned correlated response remains separate."}),
                )
            }
            MessagingAction::Execution {
                location,
                expected_revision,
            } => {
                if let Some(location) = location {
                    if location.connector.as_ref().is_some_and(|id| {
                        !self
                            .available_grants()
                            .iter()
                            .any(|grant| &grant.connector == id)
                    }) {
                        return Err(MessagingError::Denied);
                    }
                    store
                        .set_execution_location(
                            &self.source,
                            &location,
                            expected_revision
                                .ok_or(MessagingError::Invalid("expected_revision is required"))?,
                        )
                        .await?;
                }
                let (location, revision) = store.execution_location(&self.source).await?;
                Ok(
                    json!({"location":location,"revision":revision,"note":"Default for future work; each actual connector call records its own placement."}),
                )
            }
        }
    }
}
