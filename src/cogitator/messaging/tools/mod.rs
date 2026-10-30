pub(in crate::cogitator::messaging) mod authority;
mod busy;
mod control;
mod inspection;

use std::{
    convert::Infallible,
    sync::{Arc, Weak},
};

use chrono::Utc;
use octo_core::CogitatorContext;
use octo_history::{AppendRequest, SessionId, SessionKind};
use rig::{completion::ToolDefinition, tool::Tool};
use serde::Deserialize;
use serde_json::{json, to_value, Value};

use self::inspection::InspectionRequest;
use super::{routes::RuntimeRoutes, MessagingRuntime};
use crate::{
    cogitator::AlbertCogitator,
    history::assistant_turn,
    messaging::{
        contract::{
            ContextAttachment, DelegationCeiling, DestinationSpec, ExecutionLocation, MessageDraft,
            MessageId, MessageIntent, ReportClass, TrustedSource, WorkRef, WorkReportPolicy,
        },
        MessagingError, MessagingResult,
    },
    sessions::SessionContext,
};

#[derive(Clone)]
pub(in crate::cogitator) struct SessionMessagingTool {
    host: Weak<AlbertCogitator>,
    ctx: CogitatorContext,
    runtime: Arc<MessagingRuntime>,
    source: TrustedSource,
}
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::cogitator) enum MessagingAction {
    Discover {
        limit: Option<usize>,
        before_id: Option<i64>,
        through_id: Option<i64>,
        route_offset: Option<usize>,
    },
    Inspect {
        session_id: SessionId,
        limit: Option<usize>,
        before_id: Option<i64>,
        through_id: Option<i64>,
        record_id: Option<i64>,
        text_offset: Option<usize>,
    },
    Send {
        #[serde(flatten)]
        message: SendProposal,
    },
    Status {
        message_id: MessageId,
    },
    Cancel {
        message_id: MessageId,
    },
    Steer {
        message_id: MessageId,
        content: String,
    },
    Report {
        content: String,
    },
    Reports {
        message_id: Option<MessageId>,
        policy: Option<WorkReportPolicy>,
    },
    Execution {
        location: Option<ExecutionLocation>,
        expected_revision: Option<i64>,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::cogitator) struct SendProposal {
    destination: DestinationSpec,
    intent: MessageIntent,
    content: String,
    timeout_secs: Option<u64>,
    ceiling: Option<DelegationCeiling>,
    policy: Option<WorkReportPolicy>,
    #[serde(default)]
    context: Vec<ContextAttachment>,
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
    async fn admit(
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
    fn prepare(&self, message: SendProposal) -> MessagingResult<MessageDraft> {
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
impl Tool for SessionMessagingTool {
    const NAME: &'static str = "session_messaging";
    type Error = Infallible;
    type Args = MessagingAction;
    type Output = Value;
    async fn definition(&self, _: String) -> ToolDefinition {
        ToolDefinition { name: Self::NAME.into(), description: "Discover/inspect your durable private sessions with bounded text excerpts (record_id + text_offset continues an oversized record; media/encoded data are omitted), send addressed notices/questions/explicitly bounded work, inspect asynchronous status, cancel or steer one exact message, configure this work's independent intermediate/final report destinations, and inspect/set host execution defaults. Questions have no tools. Notice/Response are passive. Sending returns an acceptance ID immediately; acceptance is not physical delivery. Never claim delivery from acceptance. No temporary exports: Save first. Actor, source, current work and return address are host supplied. Control targets only one message, never unrelated children.".into(),
            parameters: json!({"type":"object","required":["action"],"properties":{
                "action":{"type":"string","enum":["discover","inspect","send","status","cancel","steer","report","reports","execution"]},
                "limit":{"type":"integer"},"before_id":{"type":"integer"},"through_id":{"type":"integer"},
                "record_id":{"type":"integer"},"text_offset":{"type":"integer"},"route_offset":{"type":"integer"},"session_id":{"type":"string"},"message_id":{"type":"string"},"content":{"type":"string"},
                "intent":{"type":"string","enum":["notice","question","work_request"]},"timeout_secs":{"type":"integer"},
                "destination":{"type":"object","description":"{kind:fixed_session,session:UUID,display?:{connector,channel}} or {kind:channel_active,route:{connector,channel}}"},
                "ceiling":{"type":"object","description":"WorkRequest only: {grants:[{connector,kinds:[exact concrete event kinds]}],max_tool_calls,max_output_bytes}. Must be within discovered initiating capabilities."},
                "context":{"type":"array","items":{"type":"object"}},"policy":{"type":"object","description":"On send: revision1 policy atomically binds to new question/work_request. On reports: omit policy to get or set a new revision; optional message_id targets that admitted work, otherwise current native work."},
                "location":{"type":"object","description":"Omit to get; set {machine:host,connector:ID|null}"},"expected_revision":{"type":"integer"}
            },"additionalProperties":false}) }
    }
    async fn call(&self, action: MessagingAction) -> Result<Value, Infallible> {
        Ok(match self.invoke(action).await {
            Ok(value) => value,
            Err(error) => {
                json!({"accepted":false,"status":"not_applied","error":error.to_string()})
            }
        })
    }
}
