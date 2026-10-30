use chrono::Utc;
use octo_core::{CogitatorContext, Envelope};
use octo_history::{AppendRequest, SessionKind};
use serde::Serialize;
use serde_json::to_vec;
use sha2::{Digest, Sha256};
use tracing::warn;

use super::{routes::RuntimeRoutes, LogicalWork};
use crate::{
    cogitator::{channel_of, AlbertCogitator},
    history::Turn,
    messaging::{
        contract::{
            ExecutionLocation, MessageDraft, MessageId, MessageIntent, ReportClass, TrustedSource,
            WorkRef,
        },
        store::ReportAdmission,
    },
    sessions::SessionContext,
};

pub(in crate::cogitator::messaging) fn report_id<T: Serialize>(identity: &T) -> MessageId {
    let digest = format!(
        "{:x}",
        Sha256::digest(to_vec(identity).expect("host report identity serializes"))
    );
    MessageId::try_from(digest[..32].to_owned()).expect("host hex ID")
}
impl AlbertCogitator {
    pub(in crate::cogitator) async fn commit_native_report(
        &self,
        incoming: &Envelope,
        scope: &str,
        records: Vec<Turn>,
        answer: &str,
        ctx: &CogitatorContext,
    ) -> Result<Option<ReportAdmission>, String> {
        let channel = channel_of(incoming);
        let (Some(runtime), Some(context)) =
            (&self.messaging, incoming.context.get::<SessionContext>())
        else {
            self.append_host(&channel, scope, "final", records).await?;
            return Ok(None);
        };
        if !context.is_private() || context.session().kind != SessionKind::Permanent {
            self.append_host(&channel, scope, "final", records).await?;
            return Ok(None);
        }
        let work = incoming
            .context
            .get::<LogicalWork>()
            .map(|work| work.0.clone())
            .unwrap_or_else(|| WorkRef::Turn(incoming.id.to_string()));
        let source = TrustedSource::current(context, work.clone(), ExecutionLocation::default())
            .map_err(|error| error.to_string())?;
        let policy = runtime
            .store
            .report_policy(&source)
            .await
            .map_err(|error| error.to_string())?;
        let Some(destination) = policy
            .as_ref()
            .and_then(|policy| policy.final_result.clone())
        else {
            self.append_host(&channel, scope, "final", records).await?;
            if policy.is_some() {
                if let Err(error) = runtime.store.clear_report_policy(&source).await {
                    warn!(%error, "completed report policy cleanup failed");
                }
            }
            return Ok(None);
        };
        let now = Utc::now().timestamp_millis();
        let report = MessageDraft {
            id: report_id(&(context.session_id(), &work, "final")),
            destination,
            intent: MessageIntent::Notice,
            content: answer.into(),
            correlation: None,
            causation: None,
            deadline_unix_ms: now + runtime.settings.max_work_secs as i64 * 1000,
            hops_remaining: runtime.settings.limits.max_hops,
            context: vec![],
            ceiling: None,
        };
        let request = AppendRequest {
            namespace: "albert/host".into(),
            idempotency_key: format!("{scope}/final"),
            digest: format!(
                "{:x}",
                Sha256::digest(to_vec(&records).map_err(|error| error.to_string())?)
            ),
            turns: records,
        };
        let receipt = runtime
            .store
            .commit_report(
                &source,
                &request,
                ReportClass::Final,
                Some(&report),
                &RuntimeRoutes::with_registry(ctx, runtime.adapters.clone()),
                now,
            )
            .await
            .map_err(|error| error.to_string())?;
        if let Err(error) = runtime.store.clear_report_policy(&source).await {
            warn!(%error, "completed report policy cleanup failed");
        }
        Ok(Some(receipt.report))
    }
}
