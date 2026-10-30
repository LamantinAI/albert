use octo_history::AppendRequest;
use serde_json::{from_str, to_string};
use sqlx::{query, query_as, Acquire, Sqlite, Transaction};

use super::{
    persistence::fingerprint, MessageStore, ReportAdmission, ReportCommitReceipt, WorkCompletion,
};
use crate::messaging::{
    contract::{
        ContentRef, DestinationSpec, MessageDraft, MessageEnvelope, MessageId, MessageIntent,
        ReportClass, TrustedSource, WorkReportPolicy,
    },
    policy::RouteGuard,
    MessagingError, MessagingResult,
};

impl MessageStore {
    /// Native/root work has no received-message inbox. Commit its existing
    /// canonical final/intermediate append and optional report in the same tx.
    /// The additional canonical metadata record binds class, actual placement,
    /// work reference and report semantics across retries without a task catalog.
    pub async fn commit_report(
        &self,
        source: &TrustedSource,
        request: &AppendRequest,
        class: ReportClass,
        report: Option<&MessageDraft>,
        guard: &dyn RouteGuard,
        now: i64,
    ) -> MessagingResult<ReportCommitReceipt> {
        let reports: Vec<_> = report.cloned().into_iter().collect();
        let (result, mut outcomes) = self
            .commit_reports(source, request, class, &reports, guard, now)
            .await?;
        Ok(ReportCommitReceipt {
            result,
            report: outcomes.pop().unwrap_or(ReportAdmission::NotRequested),
        })
    }
    pub async fn commit_reports(
        &self,
        source: &TrustedSource,
        request: &AppendRequest,
        class: ReportClass,
        reports: &[MessageDraft],
        guard: &dyn RouteGuard,
        now: i64,
    ) -> MessagingResult<(ContentRef, Vec<ReportAdmission>)> {
        if reports.len() > 2 {
            return Err(MessagingError::Invalid("too many reports"));
        }
        if !source.context.is_live() {
            return Err(MessagingError::Unavailable);
        }
        if reports.iter().any(|draft| {
            !matches!(
                draft.intent,
                MessageIntent::Notice | MessageIntent::Response
            )
        }) {
            return Err(MessagingError::Invalid("report must be passive"));
        }
        let _lease = self.lifecycle.read().await;
        let mut tx = self.history.begin_journal().await?;
        if !source.context.is_live() {
            return Err(MessagingError::Unavailable);
        }
        let actor = source.context.private_user_id()?.as_str();
        self.session_in(&mut tx, source.context.session_id(), actor)
            .await?;
        let journal = source.context.journal();
        let receipt = self
            .history
            .append_session_in(&mut tx, &journal, request)
            .await?;
        let report_digest = reports
            .iter()
            .map(fingerprint)
            .collect::<MessagingResult<Vec<_>>>()?;
        let identity = fingerprint(&(&request.namespace, &request.idempotency_key))?;
        let metadata = (
            "albert.work_report.committed.v1",
            &source.work,
            &source.execution,
            class,
            &receipt.record_ids,
            report_digest,
        );
        let committed = self
            .append_receipt_in(&mut tx, &journal, &identity, "report_commit", &metadata)
            .await?;
        let mut outcomes = Vec::new();
        if committed.duplicate {
            outcomes.push(ReportAdmission::AlreadyCommitted);
        } else {
            for (index, report) in reports.iter().enumerate() {
                outcomes.push(
                    self.report_or_failure_in(
                        &mut tx,
                        source,
                        &format!("{identity}:{index}"),
                        Some(report),
                        guard,
                        now,
                    )
                    .await?,
                );
            }
        }
        tx.commit().await?;
        Ok((
            ContentRef {
                journal,
                record_id: receipt.through_id,
            },
            outcomes,
        ))
    }

    pub(super) async fn report_or_failure_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        source: &TrustedSource,
        identity: &str,
        report: Option<&MessageDraft>,
        guard: &dyn RouteGuard,
        now: i64,
    ) -> MessagingResult<ReportAdmission> {
        let Some(report) = report else {
            return Ok(ReportAdmission::NotRequested);
        };
        let mut savepoint = tx.begin().await?;
        match self
            .send_in(&mut savepoint, source, report, guard, now)
            .await
        {
            Ok(_) => {
                savepoint.commit().await?;
                Ok(ReportAdmission::Queued(report.id.clone()))
            }
            Err(error) => {
                // Local policy/lifecycle failure is a terminal report outcome,
                // not permission to lose the completed work or rerun its tools.
                // Storage/identity failures retain all-or-nothing rollback.
                let code = match error {
                    MessagingError::Denied => "denied",
                    MessagingError::SaveRequired => "save_required",
                    MessagingError::Unavailable => "unavailable",
                    MessagingError::Limit => "limit",
                    MessagingError::Invalid(_) => "invalid_report",
                    other => return Err(other),
                };
                savepoint.rollback().await?;
                let failure = ("albert.work_report.failed.v1", &report.id, code);
                self.append_in(
                    tx,
                    &source.context.journal(),
                    identity,
                    "report_failure",
                    &failure,
                )
                .await?;
                Ok(ReportAdmission::Failed {
                    message: report.id.clone(),
                    code,
                })
            }
        }
    }
}

impl MessageStore {
    pub async fn message_report_policy(
        &self,
        source: &TrustedSource,
        id: &MessageId,
    ) -> MessagingResult<Option<WorkReportPolicy>> {
        if !source.context.is_live() {
            return Err(MessagingError::Unavailable);
        }
        let actor = source.context.private_user_id()?.as_str();
        let _lease = self.lifecycle.read().await;
        let mut tx = self.history.begin_journal().await?;
        self.session_in(&mut tx, source.context.session_id(), actor)
            .await?;
        let row: Option<(String, Option<String>)> = query_as("SELECT m.actor_id,p.policy FROM albert_messages m LEFT JOIN albert_message_report_policies p USING(message_id) WHERE m.message_id=?")
            .bind(id.as_str()).fetch_optional(&mut *tx).await?;
        let (owner, policy) = row.ok_or(MessagingError::Unavailable)?;
        if owner != actor {
            return Err(MessagingError::Denied);
        }
        let policy = policy.map(|value| from_str(&value)).transpose()?;
        tx.commit().await?;
        Ok(policy)
    }
    pub async fn set_message_report_policy(
        &self,
        source: &TrustedSource,
        id: &MessageId,
        policy: &WorkReportPolicy,
        guard: &dyn RouteGuard,
    ) -> MessagingResult<()> {
        let _lease = self.lifecycle.read().await;
        let mut tx = self.history.begin_journal().await?;
        self.set_message_report_policy_in(&mut tx, source, id, policy, guard)
            .await?;
        tx.commit().await?;
        Ok(())
    }
    pub(super) async fn set_message_report_policy_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        source: &TrustedSource,
        id: &MessageId,
        policy: &WorkReportPolicy,
        guard: &dyn RouteGuard,
    ) -> MessagingResult<()> {
        if policy.revision < 1 || to_string(policy)?.len() > self.limits.max_metadata_bytes {
            return Err(MessagingError::Invalid("invalid report policy"));
        }
        if !source.context.is_live() {
            return Err(MessagingError::Unavailable);
        }
        let actor = source.context.private_user_id()?.as_str();
        self.session_in(tx, source.context.session_id(), actor)
            .await?;
        let row: Option<(String, Option<String>)> = query_as("SELECT m.envelope,COALESCE(i.work_state,CASE WHEN o.state='failed' THEN 'failed' ELSE 'queued' END) FROM albert_messages m JOIN albert_message_outbox o USING(message_id) LEFT JOIN albert_message_inbox i USING(message_id) WHERE m.message_id=?")
            .bind(id.as_str()).fetch_optional(&mut **tx).await?;
        let (envelope, state) = row.ok_or(MessagingError::Unavailable)?;
        let envelope: MessageEnvelope = from_str(&envelope)?;
        if envelope.actor != actor || !envelope.intent.schedules_work() {
            return Err(MessagingError::Denied);
        }
        if state
            .as_deref()
            .is_some_and(|state| !matches!(state, "queued" | "running"))
        {
            return Err(MessagingError::Conflict);
        }
        for target in [&policy.intermediate, &policy.final_result]
            .into_iter()
            .flatten()
        {
            self.resolve_in(tx, actor, target, guard).await?;
        }
        let changed = if policy.revision == 1 {
            query("INSERT OR IGNORE INTO albert_message_report_policies(message_id,policy,revision) VALUES(?,?,1)")
                .bind(id.as_str()).bind(to_string(policy)?).execute(&mut **tx).await?.rows_affected()
        } else {
            query("UPDATE albert_message_report_policies SET policy=?,revision=? WHERE message_id=? AND revision=?")
                .bind(to_string(policy)?).bind(policy.revision).bind(id.as_str()).bind(policy.revision-1).execute(&mut **tx).await?.rows_affected()
        };
        if changed != 1 {
            return Err(MessagingError::Conflict);
        }
        Ok(())
    }
}

impl MessageStore {
    /// The selected B-work policy is read in the same result transaction; a
    /// concurrent policy update cannot be acknowledged and then silently missed.
    pub async fn commit_message_result(
        &self,
        completion: WorkCompletion<'_>,
        response: Option<&MessageDraft>,
        guard: &dyn RouteGuard,
        report_deadline: i64,
    ) -> MessagingResult<(ContentRef, Vec<ReportAdmission>)> {
        let WorkCompletion {
            source,
            incoming,
            result,
            now,
        } = completion;
        let _lease = self.lifecycle.read().await;
        let mut tx = self.history.begin_journal().await?;
        let row: Option<(Option<i64>, Option<String>)> = query_as("SELECT result_record,result_fingerprint FROM albert_message_inbox WHERE message_id=? AND recipient_session=?")
            .bind(incoming.as_str()).bind(source.context.session_id().as_str()).fetch_optional(&mut *tx).await?;
        if let Some((Some(record), hash)) = row {
            self.session_in(
                &mut tx,
                source.context.session_id(),
                source.context.private_user_id()?.as_str(),
            )
            .await?;
            if hash.as_deref() != Some(&fingerprint(&(&source.work, &source.execution, result))?) {
                return Err(MessagingError::Conflict);
            }
            tx.commit().await?;
            return Ok((
                ContentRef {
                    journal: source.context.journal(),
                    record_id: record,
                },
                vec![ReportAdmission::AlreadyCommitted],
            ));
        }
        let policy: Option<(String,)> =
            query_as("SELECT policy FROM albert_message_report_policies WHERE message_id=?")
                .bind(incoming.as_str())
                .fetch_optional(&mut *tx)
                .await?;
        let mut reports: Vec<_> = response.cloned().into_iter().collect();
        if let Some((policy,)) = policy {
            let policy: WorkReportPolicy = from_str(&policy)?;
            if let Some(destination) = policy.final_result {
                let digest = fingerprint(&(incoming, "selected-final"))?;
                if response.is_some_and(|response| response.destination == destination) {
                    if let Some(default) = reports.first_mut() {
                        if let DestinationSpec::FixedSession { display, .. } =
                            &mut default.destination
                        {
                            *display = None;
                        }
                    }
                }
                reports.push(MessageDraft {
                    id: MessageId::try_from(digest[..32].to_owned()).expect("hex ID"),
                    destination,
                    intent: MessageIntent::Notice,
                    content: result.into(),
                    correlation: Some(incoming.clone()),
                    causation: Some(incoming.clone()),
                    deadline_unix_ms: report_deadline,
                    hops_remaining: response.map(|draft| draft.hops_remaining).unwrap_or(1),
                    context: vec![],
                    ceiling: None,
                });
            }
        }
        let receipt = self
            .commit_result_in(
                &mut tx,
                WorkCompletion {
                    source,
                    incoming,
                    result,
                    now,
                },
                &reports,
                guard,
            )
            .await?;
        tx.commit().await?;
        Ok(receipt)
    }
}

#[cfg(test)]
mod tests {
    use octo_history::{CreateSession, SessionStore, SessionUpdate};
    use sqlx::{query, query_scalar};

    use super::super::{
        tests::{draft, fixed, journal_len, setup, Guard},
        WorkCompletion, WorkState,
    };
    use crate::messaging::{
        contract::{DestinationSpec, MessageIntent, WorkReportPolicy},
        policy::MessagingLimits,
    };

    #[tokio::test]
    async fn result_and_two_pending_reports_roll_back_if_second_outbox_write_fails() {
        let (store, a, b) = setup(MessagingLimits::default()).await;
        let c = store
            .history
            .create_session(CreateSession::private("alice"))
            .await
            .unwrap();
        let incoming = draft(fixed(&b), MessageIntent::Question);
        store.send(&a, &incoming, &Guard, 1).await.unwrap();
        store.dispatch(&incoming.id, &Guard, 2).await.unwrap();
        store.claim_work(&b, &incoming.id, &Guard, 3).await.unwrap();
        store
            .set_message_report_policy(
                &a,
                &incoming.id,
                &WorkReportPolicy {
                    revision: 1,
                    intermediate: None,
                    final_result: Some(DestinationSpec::FixedSession {
                        session: c.id,
                        display: None,
                    }),
                },
                &Guard,
            )
            .await
            .unwrap();
        let mut response = draft(fixed(&a), MessageIntent::Response);
        response.causation = Some(incoming.id.clone());
        query("CREATE TRIGGER reject_second_report BEFORE INSERT ON albert_message_outbox WHEN (SELECT COUNT(*) FROM albert_message_outbox)>1 BEGIN SELECT RAISE(ABORT,'injected second report failure'); END")
            .execute(store.history.pool()).await.unwrap();
        assert!(store
            .commit_message_result(
                WorkCompletion {
                    source: &b,
                    incoming: &incoming.id,
                    result: "complete actual B result",
                    now: 4
                },
                Some(&response),
                &Guard,
                1000
            )
            .await
            .is_err());
        assert_eq!(journal_len(&store, &b).await, 1);
        assert_eq!(
            store.status(&a, &incoming.id).await.unwrap().work,
            Some(WorkState::Running)
        );
        let count: i64 = query_scalar("SELECT COUNT(*) FROM albert_messages")
            .fetch_one(store.history.pool())
            .await
            .unwrap();
        assert_eq!(count, 1);
        assert!(store
            .message_report_policy(&a, &incoming.id)
            .await
            .unwrap()
            .is_some());
        query("DROP TRIGGER reject_second_report")
            .execute(store.history.pool())
            .await
            .unwrap();
        let receipt = store
            .commit_message_result(
                WorkCompletion {
                    source: &b,
                    incoming: &incoming.id,
                    result: "complete actual B result",
                    now: 5,
                },
                Some(&response),
                &Guard,
                1000,
            )
            .await
            .unwrap();
        assert_eq!(receipt.1.len(), 2);
        let retry = store
            .commit_message_result(
                WorkCompletion {
                    source: &b,
                    incoming: &incoming.id,
                    result: "complete actual B result",
                    now: 6,
                },
                Some(&response),
                &Guard,
                1000,
            )
            .await
            .unwrap();
        assert_eq!(receipt.0, retry.0);
        assert!(store
            .commit_message_result(
                WorkCompletion {
                    source: &b,
                    incoming: &incoming.id,
                    result: "changed result",
                    now: 7
                },
                Some(&response),
                &Guard,
                1000
            )
            .await
            .is_err());
        assert!(store
            .message_report_policy(&a, &incoming.id)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn archived_optional_report_target_preserves_result_and_correlated_response() {
        let (store, a, b) = setup(MessagingLimits::default()).await;
        let c = store
            .history
            .create_session(CreateSession::private("alice"))
            .await
            .unwrap();
        let incoming = draft(fixed(&b), MessageIntent::Question);
        store.send(&a, &incoming, &Guard, 1).await.unwrap();
        store.dispatch(&incoming.id, &Guard, 2).await.unwrap();
        store.claim_work(&b, &incoming.id, &Guard, 3).await.unwrap();
        store
            .set_message_report_policy(
                &a,
                &incoming.id,
                &WorkReportPolicy {
                    revision: 1,
                    intermediate: None,
                    final_result: Some(DestinationSpec::FixedSession {
                        session: c.id.clone(),
                        display: None,
                    }),
                },
                &Guard,
            )
            .await
            .unwrap();
        store
            .history
            .update_session(
                &c.id,
                SessionUpdate {
                    expected_revision: c.revision,
                    title: None,
                    archived: Some(true),
                },
            )
            .await
            .unwrap();
        let mut response = draft(fixed(&a), MessageIntent::Response);
        response.causation = Some(incoming.id.clone());
        let receipt = store
            .commit_message_result(
                WorkCompletion {
                    source: &b,
                    incoming: &incoming.id,
                    result: "completed despite archived report route",
                    now: 4,
                },
                Some(&response),
                &Guard,
                1000,
            )
            .await
            .unwrap();
        assert_eq!(receipt.1.len(), 2);
        let status = store.status(&a, &incoming.id).await.unwrap();
        assert_eq!(status.work, Some(WorkState::Completed));
        assert!(status.result.is_some());
        let outcomes = status.report_outcomes.unwrap();
        assert_eq!(outcomes[0]["status"], "queued");
        assert_eq!(outcomes[1]["status"], "failed");
        store.dispatch(&response.id, &Guard, 5).await.unwrap();
        assert!(journal_len(&store, &a).await > 1);
    }
}
