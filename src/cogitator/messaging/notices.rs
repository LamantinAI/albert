use chrono::Utc;
use octo_core::CogitatorContext;
use octo_history::AppendRequest;
use sha2::{Digest, Sha256};
use tracing::warn;

use super::{reports::report_id, routes::RuntimeRoutes, MessagingRuntime};
use crate::{
    cogitator::AlbertCogitator,
    history::assistant_turn,
    messaging::{
        contract::{
            DestinationSpec, ExecutionLocation, MessageDraft, MessageEnvelope, MessageId,
            MessageIntent, ReportClass, TrustedSource,
        },
        MessagingError, MessagingResult,
    },
    sessions::error::SessionError,
};

impl AlbertCogitator {
    pub(in crate::cogitator::messaging) async fn message_failure_notices(
        &self,
        runtime: &MessagingRuntime,
        ctx: &CogitatorContext,
    ) {
        let Ok(notices) = runtime
            .store
            .failure_notices(runtime.settings.batch_size)
            .await
        else {
            return;
        };
        for (envelope, code) in notices {
            // Delivery failures of passive notices/responses never form a loop.
            if !envelope.intent.schedules_work() {
                let _ = runtime.store.acknowledge_failure_notice(&envelope.id).await;
                continue;
            }
            match self
                .record_message_failure(runtime, ctx, &envelope, &code)
                .await
            {
                Ok(()) => {
                    let _ = runtime.store.acknowledge_failure_notice(&envelope.id).await;
                }
                Err(error) => {
                    warn!(%error, message_id=envelope.id.as_str(), "terminal request notice could not be recorded")
                }
            }
        }
    }
    async fn record_message_failure(
        &self,
        runtime: &MessagingRuntime,
        ctx: &CogitatorContext,
        envelope: &MessageEnvelope,
        code: &str,
    ) -> MessagingResult<()> {
        let Some(service) = &self.session_service else {
            return Ok(());
        };
        let context = match service
            .messaging_context(&envelope.actor, &envelope.source, &envelope.return_address)
            .await
        {
            Ok(context) => context,
            // An ended/archived original source cannot receive a new journal
            // append; its owner can still inspect the original terminal status.
            Err(
                MessagingError::Unavailable
                | MessagingError::SaveRequired
                | MessagingError::Session(
                    SessionError::Ended
                    | SessionError::Expired
                    | SessionError::Denied
                    | SessionError::NotFound,
                ),
            ) => return Ok(()),
            Err(error) => return Err(error),
        };
        let source = TrustedSource::current(
            &context,
            envelope.work.clone(),
            ExecutionLocation::default(),
        )?;
        let content = format!("Session request {} ended with status {code}. Its recorded tool outcomes remain authoritative; partial external effects must be checked before repeating actions. No automatic replay was started.", envelope.id.as_str());
        let digest = format!(
            "{:x}",
            Sha256::digest(format!("{}:terminal-notice", envelope.id.as_str()).as_bytes())
        );
        let id = MessageId::try_from(digest[..32].to_owned()).expect("host-generated hex ID");
        let report = MessageDraft {
            id,
            destination: DestinationSpec::FixedSession {
                session: envelope.return_address.session.clone(),
                display: Some(envelope.return_address.display.clone()),
            },
            intent: MessageIntent::Response,
            content: content.clone(),
            correlation: Some(envelope.id.clone()),
            causation: Some(envelope.id.clone()),
            deadline_unix_ms: envelope
                .deadline_unix_ms
                .saturating_add(runtime.settings.max_work_secs as i64 * 1000),
            hops_remaining: 1,
            context: vec![],
            ceiling: None,
        };
        let request = AppendRequest {
            namespace: "albert.message-terminal.v1".into(),
            idempotency_key: envelope.id.as_str().into(),
            digest,
            turns: vec![assistant_turn(content)],
        };
        let mut reports = vec![report];
        if let Some(destination) = runtime
            .store
            .message_report_policy(&source, &envelope.id)
            .await?
            .and_then(|policy| policy.final_result)
        {
            let mut selected = reports[0].clone();
            if reports[0].destination == destination {
                if let DestinationSpec::FixedSession { display, .. } = &mut reports[0].destination {
                    *display = None;
                }
            }
            selected.id = report_id(&(&envelope.id, "terminal-selected-final"));
            selected.destination = destination;
            selected.intent = MessageIntent::Notice;
            reports.push(selected);
        }
        runtime
            .store
            .commit_reports(
                &source,
                &request,
                ReportClass::Final,
                &reports,
                &RuntimeRoutes::with_registry(ctx, runtime.adapters.clone()),
                Utc::now().timestamp_millis(),
            )
            .await?;
        Ok(())
    }
}
