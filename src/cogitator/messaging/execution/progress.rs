use std::{
    convert::Infallible,
    sync::{
        atomic::{AtomicU32, AtomicUsize, Ordering},
        Arc,
    },
};

use chrono::Utc;
use octo_core::CogitatorContext;
use octo_history::AppendRequest;
use rig::{completion::ToolDefinition, tool::Tool};
use serde::Deserialize;
use serde_json::{json, Value};

use super::super::{routes::RuntimeRoutes, MessagingRuntime};
use crate::{
    history::assistant_turn,
    messaging::contract::{MessageDraft, MessageId, MessageIntent, ReportClass, TrustedSource},
    messaging::MessagingError,
};

pub(super) struct MessageProgress {
    pub runtime: Arc<MessagingRuntime>,
    pub ctx: CogitatorContext,
    pub source: TrustedSource,
    pub incoming: MessageId,
    pub hops: u16,
    pub calls: Arc<AtomicU32>,
    pub bytes: Arc<AtomicUsize>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProgressArgs {
    content: String,
}
impl Tool for MessageProgress {
    const NAME: &'static str = "report_progress";
    type Error = Infallible;
    type Args = ProgressArgs;
    type Output = Value;
    async fn definition(&self, _: String) -> ToolDefinition {
        ToolDefinition { name: Self::NAME.into(), description: "Emit a deliberate intermediate summary for this admitted work using its configured report destination. This consumes the admitted tool/output budget. No destination/actor override, messaging recursion or raw tool/reasoning stream is available. Acceptance is not confirmed physical delivery.".into(),
            parameters: json!({"type":"object","properties":{"content":{"type":"string"}},"required":["content"],"additionalProperties":false}) }
    }
    async fn call(&self, args: ProgressArgs) -> Result<Value, Infallible> {
        if args.content.is_empty()
            || self.hops == 0
            || self
                .calls
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                    left.checked_sub(1)
                })
                .is_err()
            || self
                .bytes
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                    left.checked_sub(args.content.len())
                })
                .is_err()
        {
            return Ok(json!({"accepted":false,"error":"Admitted report budget exhausted."}));
        }
        let result = async {
            let policy = self.runtime.store.message_report_policy(&self.source, &self.incoming).await?;
            let Some(destination) = policy.and_then(|policy| policy.intermediate) else { return Ok(json!({"accepted":false,"error":"No intermediate destination is configured for this work."})); };
            let id = MessageId::new(); let now = Utc::now().timestamp_millis();
            let report = MessageDraft { id: id.clone(), destination, intent: MessageIntent::Notice, content: args.content.clone(),
                correlation: Some(self.incoming.clone()), causation: Some(self.incoming.clone()), deadline_unix_ms: now + self.runtime.settings.max_work_secs as i64 * 1000,
                hops_remaining: self.hops, context: vec![], ceiling: None };
            let request = AppendRequest { namespace: "albert.message-progress.v1".into(), idempotency_key: id.as_str().into(), digest: id.as_str().into(), turns: vec![assistant_turn(args.content)] };
            let receipt = self.runtime.store.commit_report(&self.source, &request, ReportClass::Intermediate, Some(&report), &RuntimeRoutes::with_registry(&self.ctx, self.runtime.adapters.clone()), now).await?;
            Ok::<_, MessagingError>(json!({"message_id":id,"report_outcome":receipt.report,"physical_delivery":"consult_status"}))
        }.await;
        Ok(result.unwrap_or_else(|error| json!({"accepted":false,"error":error.to_string()})))
    }
}
