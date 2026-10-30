use octo_history::{SessionAccess, SessionJournal};
use serde::Serialize;
use serde_json::{from_str, to_string};
use sqlx::{query, query_as, query_scalar, Sqlite, Transaction};

use super::{persistence::fingerprint, MessageStore, RecoveryItem};
use crate::messaging::{
    contract::{
        ContentRef, DelegationCeiling, MessageDraft, MessageEnvelope, MessageId, MessageIntent,
        TrustedSource,
    },
    policy::RouteGuard,
    MessagingError, MessagingResult,
};

#[derive(Clone, Debug)]
pub enum WorkAuthority {
    ContextOnly,
    Delegated(DelegationCeiling),
}

#[derive(Clone, Debug)]
pub struct AdmittedWork {
    pub envelope: MessageEnvelope,
    pub received: ContentRef,
    pub authority: WorkAuthority,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "status", content = "details", rename_all = "snake_case")]
pub enum ReportAdmission {
    NotRequested,
    Queued(MessageId),
    Failed {
        message: MessageId,
        code: &'static str,
    },
    AlreadyCommitted,
}
#[derive(Clone, Debug)]
pub struct ReportCommitReceipt {
    pub result: ContentRef,
    pub report: ReportAdmission,
}

#[derive(Clone, Copy, Debug)]
pub enum WorkTerminal {
    Interrupted,
    Cancelled,
    Expired,
    ExecutionFailed,
    Revoked,
}
impl WorkTerminal {
    fn code(self) -> &'static str {
        match self {
            Self::Interrupted => "interrupted_unknown",
            Self::Cancelled => "cancelled",
            Self::Expired => "expired",
            Self::ExecutionFailed => "execution_failed",
            Self::Revoked => "revoked",
        }
    }
}

#[derive(Clone, Copy)]
pub struct WorkCompletion<'a> {
    pub source: &'a TrustedSource,
    pub incoming: &'a MessageId,
    pub result: &'a str,
    pub now: i64,
}

impl MessageStore {
    /// S2 must build an ordinary restricted tool context from authority. This
    /// claim supplies data and a ceiling; it does not manufacture native ingress,
    /// owner role or approval rights. Question always gets ContextOnly authority.
    pub async fn claim_work(
        &self,
        source: &TrustedSource,
        id: &MessageId,
        guard: &dyn RouteGuard,
        now: i64,
    ) -> MessagingResult<AdmittedWork> {
        let _lease = self.lifecycle.read().await;
        let mut tx = self.history.begin_journal().await?;
        let actor = source.context.private_user_id()?.as_str();
        if !source.context.is_live() {
            return Err(MessagingError::Unavailable);
        }
        self.session_in(&mut tx, source.context.session_id(), actor)
            .await?;
        let (envelope, resolved, record, state): (String, Option<String>, Option<i64>, String) = query_as(
            "SELECT m.envelope,o.resolved,i.recipient_record,i.work_state FROM albert_messages m JOIN albert_message_outbox o USING(message_id) JOIN albert_message_inbox i USING(message_id) WHERE message_id=? AND i.recipient_session=?")
            .bind(id.as_str()).bind(source.context.session_id().as_str()).fetch_optional(&mut *tx).await?.ok_or(MessagingError::Unavailable)?;
        if state != "queued" {
            return Err(MessagingError::Conflict);
        }
        let envelope: MessageEnvelope = from_str(&envelope)?;
        if envelope.actor != actor {
            return Err(MessagingError::Denied);
        }
        self.check_envelope_in(&mut tx, &envelope, now).await?;
        let target = from_str(&resolved.ok_or(MessagingError::Conflict)?)?;
        self.recheck_target_in(&mut tx, actor, &target, guard)
            .await?;
        let received = ContentRef {
            journal: SessionJournal::Root(source.context.session_id().clone()),
            record_id: record.ok_or(MessagingError::Conflict)?,
        };
        let authority = match envelope.intent {
            MessageIntent::Question => WorkAuthority::ContextOnly,
            MessageIntent::WorkRequest => {
                WorkAuthority::Delegated(envelope.ceiling.clone().ok_or(MessagingError::Conflict)?)
            }
            _ => return Err(MessagingError::Conflict),
        };
        query("UPDATE albert_message_inbox SET work_state='running' WHERE message_id=? AND recipient_session=? AND work_state='queued'")
            .bind(id.as_str()).bind(source.context.session_id().as_str()).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(AdmittedWork {
            envelope,
            received,
            authority,
        })
    }

    /// Result journal append, completed state, and optional pending response all
    /// commit or roll back together. Use current work's authenticated context.
    /// No external publish happens here; dispatch/send_started happen afterward.
    #[cfg(test)]
    pub async fn commit_result(
        &self,
        source: &TrustedSource,
        incoming: &MessageId,
        result: &str,
        report: Option<&MessageDraft>,
        guard: &dyn RouteGuard,
        now: i64,
    ) -> MessagingResult<ContentRef> {
        let reports = report.cloned().into_iter().collect::<Vec<_>>();
        self.commit_result_reports(source, incoming, result, &reports, guard, now)
            .await
            .map(|receipt| receipt.0)
    }
    #[cfg(test)]
    pub async fn commit_result_reports(
        &self,
        source: &TrustedSource,
        incoming: &MessageId,
        result: &str,
        reports: &[MessageDraft],
        guard: &dyn RouteGuard,
        now: i64,
    ) -> MessagingResult<(ContentRef, Vec<ReportAdmission>)> {
        let _lease = self.lifecycle.read().await;
        let mut tx = self.history.begin_journal().await?;
        let result = self
            .commit_result_in(
                &mut tx,
                WorkCompletion {
                    source,
                    incoming,
                    result,
                    now,
                },
                reports,
                guard,
            )
            .await?;
        tx.commit().await?;
        Ok(result)
    }
    pub(super) async fn commit_result_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        completion: WorkCompletion<'_>,
        reports: &[MessageDraft],
        guard: &dyn RouteGuard,
    ) -> MessagingResult<(ContentRef, Vec<ReportAdmission>)> {
        let WorkCompletion {
            source,
            incoming,
            result,
            now,
        } = completion;
        if reports.len() > 2 {
            return Err(MessagingError::Invalid("too many final reports"));
        }
        if result.is_empty() || result.len() > self.limits.max_payload_bytes {
            return Err(MessagingError::Limit);
        }
        if !source.context.is_live() {
            return Err(MessagingError::Unavailable);
        }
        for report in reports {
            if !matches!(
                report.intent,
                MessageIntent::Notice | MessageIntent::Response
            ) || report.causation.as_ref() != Some(incoming)
            {
                return Err(MessagingError::Invalid(
                    "report must be a correlated passive message",
                ));
            }
        }
        let actor = source.context.private_user_id()?.as_str();
        self.session_in(tx, source.context.session_id(), actor)
            .await?;
        let (state, old_result): (String, Option<i64>) = query_as("SELECT work_state,result_record FROM albert_message_inbox WHERE message_id=? AND recipient_session=?")
            .bind(incoming.as_str()).bind(source.context.session_id().as_str()).fetch_optional(&mut **tx).await?.ok_or(MessagingError::Unavailable)?;
        if !matches!(state.as_str(), "running" | "completed") {
            return Err(MessagingError::Conflict);
        }
        let report_digest = reports
            .iter()
            .map(fingerprint)
            .collect::<MessagingResult<Vec<_>>>()?;
        let record = (
            "albert.session_work.result.v1",
            incoming,
            &source.work,
            &source.execution,
            result,
            report_digest,
        );
        let reference = self
            .append_in(
                tx,
                &source.context.journal(),
                incoming.as_str(),
                "result",
                &record,
            )
            .await?;
        if old_result.is_some_and(|old| old != reference.record_id) {
            return Err(MessagingError::Conflict);
        }
        query("UPDATE albert_message_inbox SET work_state='completed',result_record=?,result_fingerprint=? WHERE message_id=? AND recipient_session=?")
            .bind(reference.record_id).bind(fingerprint(&(&source.work, &source.execution, result))?).bind(incoming.as_str()).bind(source.context.session_id().as_str()).execute(&mut **tx).await?;
        let mut outcomes = Vec::new();
        if old_result.is_none() {
            for (index, report) in reports.iter().enumerate() {
                let identity = format!("{}:report-{index}", incoming.as_str());
                outcomes.push(
                    self.report_or_failure_in(tx, source, &identity, Some(report), guard, now)
                        .await?,
                );
            }
            query("UPDATE albert_message_inbox SET report_outcomes=? WHERE message_id=? AND recipient_session=?")
                .bind(to_string(&outcomes)?).bind(incoming.as_str()).bind(source.context.session_id().as_str()).execute(&mut **tx).await?;
            query("DELETE FROM albert_message_report_policies WHERE message_id=?")
                .bind(incoming.as_str())
                .execute(&mut **tx)
                .await?;
        }
        Ok((reference, outcomes))
    }

    /// Exact work termination, separate from physical delivery state. Host
    /// cancellation is scoped to this admitted message and recipient only.
    pub async fn finish_work(
        &self,
        source: &TrustedSource,
        id: &MessageId,
        reason: WorkTerminal,
    ) -> MessagingResult<()> {
        let _lease = self.lifecycle.read().await;
        let mut tx = self.history.begin_journal().await?;
        // Terminal control metadata may be recorded after a lease closes. It
        // grants no content write/execution and still requires canonical owner.
        let actor = source.context.private_user_id()?.as_str();
        let session = self
            .history
            .get_session_in(&mut tx, source.context.session_id())
            .await?
            .ok_or(MessagingError::Unavailable)?;
        if !matches!(session.access, SessionAccess::Private { ref owner } if owner == actor) {
            return Err(MessagingError::Denied);
        }
        let changed = query("UPDATE albert_message_inbox SET work_state=?,failure_code=? WHERE message_id=? AND recipient_session=? AND work_state IN ('queued','running')")
            .bind(match reason { WorkTerminal::Cancelled => "cancelled", WorkTerminal::Interrupted => "unknown", _ => "failed" })
            .bind(reason.code()).bind(id.as_str()).bind(source.context.session_id().as_str())
            .execute(&mut *tx).await?.rows_affected();
        if changed != 1 {
            return Err(MessagingError::Conflict);
        }
        tx.commit().await?;
        Ok(())
    }

    /// Run once at startup before dispatch/scheduling. Running work and started
    /// transport become unknown; neither is eligible for blind side-effect replay.
    pub async fn recover(&self, limit: usize) -> MessagingResult<Vec<RecoveryItem>> {
        if !(1..=1000).contains(&limit) {
            return Err(MessagingError::Invalid("invalid recovery page size"));
        }
        let mut tx = self.history.begin_journal().await?;
        query("UPDATE albert_message_inbox SET work_state='unknown' WHERE work_state='running' AND result_record IS NULL")
            .execute(&mut *tx).await?;
        query("UPDATE albert_message_outbox SET state='unknown',failure_code='transport_unknown' WHERE state='started'")
            .execute(&mut *tx).await?;
        tx.commit().await?;
        self.recovery_page(None, limit).await
    }

    /// Delete bounded, old terminal control metadata only. Canonical journal
    /// content/receipts are retained. Unknown work/transport is never pruned.
    pub async fn cleanup_terminal(
        &self,
        before_unix_ms: i64,
        now: i64,
        limit: usize,
    ) -> MessagingResult<u64> {
        if !(1..=1000).contains(&limit) {
            return Err(MessagingError::Invalid("invalid cleanup page size"));
        }
        let mut tx = self.history.begin_journal().await?;
        let ids: Vec<String> = query_scalar("SELECT m.message_id FROM albert_messages m JOIN albert_message_outbox o USING(message_id) WHERE m.created_at<? AND m.deadline_unix_ms<=? AND o.state IN ('delivered','failed') AND NOT EXISTS (SELECT 1 FROM albert_message_inbox i WHERE i.message_id=m.message_id AND i.work_state IN ('queued','running','unknown')) ORDER BY m.created_at LIMIT ?")
            .bind(before_unix_ms).bind(now).bind(limit as i64).fetch_all(&mut *tx).await?;
        for id in &ids {
            self.delete_control_in(&mut tx, id).await?;
        }
        tx.commit().await?;
        Ok(ids.len() as u64)
    }
}

#[cfg(test)]
mod tests {
    use octo_history::{AppendRequest, Turn};

    use super::super::{
        tests::{display, draft, fixed, journal_len, setup, Guard, Revoked},
        WorkState,
    };
    use super::{ReportAdmission, WorkTerminal};
    use crate::messaging::{
        contract::{DestinationSpec, MessageIntent, ReportClass},
        policy::MessagingLimits,
    };

    #[tokio::test]
    async fn native_result_survives_report_revocation_and_retry_does_not_resend() {
        let (store, a, b) = setup(MessagingLimits::default()).await;
        let request = AppendRequest {
            namespace: "native.final".into(),
            idempotency_key: "turn-1".into(),
            digest: "result".into(),
            turns: vec![Turn::assistant("completed native result")],
        };
        let mut proposal = draft(fixed(&b), MessageIntent::Notice);
        proposal.destination = DestinationSpec::FixedSession {
            session: b.context.session_id().clone(),
            display: Some(display()),
        };
        let receipt = store
            .commit_report(
                &a,
                &request,
                ReportClass::Final,
                Some(&proposal),
                &Revoked,
                1,
            )
            .await
            .unwrap();
        assert!(matches!(
            receipt.report,
            ReportAdmission::Failed { code: "denied", .. }
        ));
        assert_eq!(journal_len(&store, &a).await, 3);
        let retry = store
            .commit_report(&a, &request, ReportClass::Final, Some(&proposal), &Guard, 2)
            .await
            .unwrap();
        assert!(matches!(retry.report, ReportAdmission::AlreadyCommitted));
        assert_eq!(retry.result, receipt.result);
        assert_eq!(journal_len(&store, &a).await, 3);
        assert!(store.status(&a, &proposal.id).await.is_err());
    }

    #[tokio::test]
    async fn termination_distinguishes_scoped_cancel_from_execution_failure() {
        let (store, a, b) = setup(MessagingLimits::default()).await;
        let first = draft(fixed(&b), MessageIntent::Question);
        let second = draft(fixed(&b), MessageIntent::Question);
        for proposal in [&first, &second] {
            store.send(&a, proposal, &Guard, 1).await.unwrap();
        }
        store
            .finish_work(&b, &first.id, WorkTerminal::Cancelled)
            .await
            .unwrap();
        assert_eq!(
            store.status(&a, &first.id).await.unwrap().work,
            Some(WorkState::Cancelled)
        );
        assert_eq!(
            store.status(&a, &second.id).await.unwrap().work,
            Some(WorkState::Queued)
        );
        store
            .finish_work(&b, &second.id, WorkTerminal::ExecutionFailed)
            .await
            .unwrap();
        assert_eq!(
            store
                .status(&a, &second.id)
                .await
                .unwrap()
                .work_failure_code
                .as_deref(),
            Some("execution_failed")
        );
    }
}
