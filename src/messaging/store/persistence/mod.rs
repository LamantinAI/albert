use octo_history::{AppendReceipt, AppendRequest, Session, SessionId, SessionJournal, Turn};
use serde::{Deserialize, Serialize};
use serde_json::{from_str, to_string, to_vec, Value};
use sha2::{Digest, Sha256};
use sqlx::{query, query_as, query_scalar, Sqlite, Transaction};

use super::{AdmissionReceipt, MessageStore};
use crate::messaging::{
    contract::{
        ContentRef, ContextAttachment, DestinationSpec, DisplayRoute, MessageDraft,
        MessageEnvelope, ResolvedDestination, ReturnAddress, TrustedSource, WorkReportPolicy,
        CONTRACT_VERSION, MESSAGE_MARKER,
    },
    policy::{draft, private_session, RouteGuard},
    MessagingError, MessagingResult,
};

/// Host-authored record, explicitly attributed as session message data. Neither
/// this marker nor any quoted payload becomes a trusted human ingress/owner role.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MessageRecord {
    pub record_type: String,
    pub envelope: MessageEnvelope,
    pub content: String,
    pub context: Vec<ContextAttachment>,
    pub original: Option<ContentRef>,
}

impl MessageStore {
    #[cfg(test)]
    pub async fn send(
        &self,
        source: &TrustedSource,
        proposal: &MessageDraft,
        guard: &dyn RouteGuard,
        now: i64,
    ) -> MessagingResult<AdmissionReceipt> {
        self.send_with_policy(source, proposal, None, guard, now)
            .await
    }
    pub async fn send_with_policy(
        &self,
        source: &TrustedSource,
        proposal: &MessageDraft,
        policy: Option<&WorkReportPolicy>,
        guard: &dyn RouteGuard,
        now: i64,
    ) -> MessagingResult<AdmissionReceipt> {
        draft(source, proposal, &self.limits, now)?;
        if policy.is_some_and(|policy| policy.revision != 1 || !proposal.intent.schedules_work()) {
            return Err(MessagingError::Invalid(
                "new work requires report policy revision 1",
            ));
        }
        let _lease = self.lifecycle.read().await;
        let mut tx = self.history.begin_journal().await?;
        if let Some(policy) = policy {
            let actor = source.context.private_user_id()?.as_str();
            if to_string(policy)?.len() > self.limits.max_metadata_bytes {
                return Err(MessagingError::Limit);
            }
            for target in [&policy.intermediate, &policy.final_result]
                .into_iter()
                .flatten()
            {
                self.resolve_in(&mut tx, actor, target, guard).await?;
            }
        }
        let receipt = self
            .send_in_with_policy(&mut tx, source, proposal, policy, guard, now)
            .await?;
        if !receipt.duplicate {
            if let Some(policy) = policy {
                self.set_message_report_policy_in(&mut tx, source, &proposal.id, policy, guard)
                    .await?;
            }
        }
        tx.commit().await?;
        Ok(receipt)
    }

    pub(super) async fn send_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        source: &TrustedSource,
        proposal: &MessageDraft,
        guard: &dyn RouteGuard,
        now: i64,
    ) -> MessagingResult<AdmissionReceipt> {
        self.send_in_with_policy(tx, source, proposal, None, guard, now)
            .await
    }
    async fn send_in_with_policy(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        source: &TrustedSource,
        proposal: &MessageDraft,
        policy: Option<&WorkReportPolicy>,
        guard: &dyn RouteGuard,
        now: i64,
    ) -> MessagingResult<AdmissionReceipt> {
        draft(source, proposal, &self.limits, now)?;
        let actor = source.context.private_user_id()?.as_str();
        self.session_in(tx, source.context.session_id(), actor)
            .await?;
        let envelope = MessageEnvelope {
            version: CONTRACT_VERSION,
            id: proposal.id.clone(),
            actor: actor.into(),
            source: source.context.session_id().clone(),
            work: source.work.clone(),
            execution: source.execution.clone(),
            intended: proposal.destination.clone(),
            intent: proposal.intent,
            correlation: proposal.correlation.clone(),
            causation: proposal.causation.clone(),
            deadline_unix_ms: proposal.deadline_unix_ms,
            hops_remaining: proposal.hops_remaining,
            return_address: ReturnAddress {
                session: source.context.session_id().clone(),
                display: DisplayRoute {
                    connector: source.context.reply_route().connector().into(),
                    channel: source.context.reply_route().channel().into(),
                },
            },
            ceiling: proposal.ceiling.clone(),
            admission_report_policy: policy.cloned(),
        };
        let envelope_json = to_string(&envelope)?;
        if envelope_json.len() > self.limits.max_metadata_bytes {
            return Err(MessagingError::Limit);
        }
        // Validate every attachment's canonical session ownership, including children.
        // References are descriptive; model-supplied excerpts confer no authority.
        for attachment in &proposal.context {
            self.session_in(tx, attachment.reference.journal.session_id(), actor)
                .await?;
        }
        let record = MessageRecord {
            record_type: "albert.session_message.sent.v1".into(),
            envelope,
            content: proposal.content.clone(),
            context: proposal.context.clone(),
            original: None,
        };
        let digest = fingerprint(&record)?;
        let previous: Option<(String, i64)> =
            query_as("SELECT digest, source_record FROM albert_messages WHERE message_id=?")
                .bind(proposal.id.as_str())
                .fetch_optional(&mut **tx)
                .await?;
        if let Some((old, record_id)) = previous {
            if old != digest {
                return Err(MessagingError::Conflict);
            }
            return Ok(AdmissionReceipt {
                source: ContentRef {
                    journal: source.context.journal(),
                    record_id,
                },
                duplicate: true,
            });
        }
        // Active targets are validated now for privacy/temporary export, then
        // resolved again and pinned only at first dispatch.
        let target = self
            .resolve_in(tx, actor, &proposal.destination, guard)
            .await?;
        self.capacity_in(tx, &target.session).await?;
        let reference = self
            .append_in(
                tx,
                &source.context.journal(),
                proposal.id.as_str(),
                "source",
                &record,
            )
            .await?;
        query("INSERT INTO albert_messages(message_id,actor_id,source_session,source_record,envelope,digest,created_at,deadline_unix_ms) VALUES(?,?,?,?,?,?,?,?)")
            .bind(proposal.id.as_str()).bind(actor).bind(source.context.session_id().as_str())
            .bind(reference.record_id).bind(envelope_json).bind(digest).bind(now).bind(proposal.deadline_unix_ms).execute(&mut **tx).await?;
        let resolved = match proposal.destination {
            DestinationSpec::FixedSession { .. } => {
                self.admit_in(tx, &record.envelope, &target).await?;
                Some(to_string(&target)?)
            }
            DestinationSpec::ChannelActive { .. } => None,
        };
        query("INSERT INTO albert_message_outbox(message_id,resolved,state) VALUES(?,?,'pending')")
            .bind(proposal.id.as_str())
            .bind(resolved)
            .execute(&mut **tx)
            .await?;
        Ok(AdmissionReceipt {
            source: reference,
            duplicate: false,
        })
    }

    pub(super) async fn session_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        id: &SessionId,
        actor: &str,
    ) -> MessagingResult<Session> {
        let session = self.history.get_session_in(tx, id).await?;
        let marker: Option<(String, Option<String>)> =
            query_as("SELECT state,owner_user_id FROM albert_session_markers WHERE session_id=?")
                .bind(id.as_str())
                .fetch_optional(&mut **tx)
                .await?;
        let Some(session) = session else {
            if marker.as_ref().is_some_and(|(state, owner)| {
                state == "temporary" && owner.as_deref() == Some(actor)
            }) {
                return Err(MessagingError::SaveRequired);
            }
            return Err(MessagingError::Unavailable);
        };
        private_session(&session, actor)?;
        match marker.as_ref().map(|(state, _)| state.as_str()) {
            Some("temporary") => return Err(MessagingError::SaveRequired),
            Some("closed" | "expired") => return Err(MessagingError::Unavailable),
            _ => {}
        }
        Ok(session)
    }

    pub(super) async fn resolve_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        actor: &str,
        destination: &DestinationSpec,
        guard: &dyn RouteGuard,
    ) -> MessagingResult<ResolvedDestination> {
        let (id, display) = match destination {
            DestinationSpec::FixedSession { session, display } => {
                (session.clone(), display.clone())
            }
            DestinationSpec::ChannelActive { route } => (
                self.route_in(tx, actor, route, guard).await?,
                Some(route.clone()),
            ),
        };
        self.session_in(tx, &id, actor).await?;
        if let Some(route) = &display {
            self.verify_route_in(tx, actor, route, guard).await?;
        }
        Ok(ResolvedDestination {
            session: id,
            display,
        })
    }

    pub(super) async fn verify_route_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        actor: &str,
        route: &DisplayRoute,
        guard: &dyn RouteGuard,
    ) -> MessagingResult<()> {
        guard.check(actor, route)?;
        let bound: i64 = query_scalar("SELECT COUNT(*) FROM albert_private_session_routes WHERE connector_id=? AND channel_id=? AND user_id=?")
            .bind(&route.connector).bind(&route.channel).bind(actor).fetch_one(&mut **tx).await?;
        if bound != 1 {
            return Err(MessagingError::Denied);
        }
        Ok(())
    }

    pub(super) async fn route_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        actor: &str,
        route: &DisplayRoute,
        guard: &dyn RouteGuard,
    ) -> MessagingResult<SessionId> {
        guard.check(actor, route)?;
        let active: Option<String> = query_scalar("SELECT session_id FROM albert_private_session_routes WHERE connector_id=? AND channel_id=? AND user_id=?")
            .bind(&route.connector).bind(&route.channel).bind(actor).fetch_optional(&mut **tx).await?;
        let id = SessionId::parse(&active.ok_or(MessagingError::Denied)?)?;
        self.session_in(tx, &id, actor).await?;
        Ok(id)
    }

    pub(super) async fn capacity_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        recipient: &SessionId,
    ) -> MessagingResult<()> {
        let total: i64 = query_scalar("SELECT COUNT(*) FROM albert_messages")
            .fetch_one(&mut **tx)
            .await?;
        let outbox: i64 = query_scalar(
            "SELECT COUNT(*) FROM albert_message_outbox WHERE state IN ('pending','retry','started')",
        )
        .fetch_one(&mut **tx)
        .await?;
        if total >= self.limits.max_messages || outbox >= self.limits.max_outbox_pending {
            return Err(MessagingError::Limit);
        }
        self.inbox_capacity_in(tx, recipient).await
    }

    pub(super) async fn inbox_capacity_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        recipient: &SessionId,
    ) -> MessagingResult<()> {
        let pending: i64 = query_scalar("SELECT COUNT(*) FROM albert_message_inbox WHERE recipient_session=? AND work_state IN ('queued','running')")
            .bind(recipient.as_str()).fetch_one(&mut **tx).await?;
        if pending >= self.limits.max_inbox_pending {
            return Err(MessagingError::Limit);
        }
        Ok(())
    }

    pub(super) async fn admit_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        envelope: &MessageEnvelope,
        target: &ResolvedDestination,
    ) -> MessagingResult<()> {
        let exists: i64 = query_scalar(
            "SELECT COUNT(*) FROM albert_message_inbox WHERE message_id=? AND recipient_session=?",
        )
        .bind(envelope.id.as_str())
        .bind(target.session.as_str())
        .fetch_one(&mut **tx)
        .await?;
        if exists != 0 {
            return Ok(());
        }
        self.inbox_capacity_in(tx, &target.session).await?;
        let state = if envelope.intent.schedules_work() {
            "queued"
        } else {
            "passive"
        };
        query("INSERT INTO albert_message_inbox(message_id,recipient_session,work_state) VALUES(?,?,?)")
            .bind(envelope.id.as_str()).bind(target.session.as_str()).bind(state).execute(&mut **tx).await?;
        Ok(())
    }

    pub(super) async fn append_in<T: Serialize>(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        journal: &SessionJournal,
        id: &str,
        phase: &str,
        record: &T,
    ) -> MessagingResult<ContentRef> {
        let receipt = self
            .append_receipt_in(tx, journal, id, phase, record)
            .await?;
        Ok(ContentRef {
            journal: journal.clone(),
            record_id: receipt.through_id,
        })
    }

    pub(super) async fn append_receipt_in<T: Serialize>(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        journal: &SessionJournal,
        id: &str,
        phase: &str,
        record: &T,
    ) -> MessagingResult<AppendReceipt> {
        let request = AppendRequest {
            namespace: "albert.session-messaging.v1".into(),
            idempotency_key: format!("{id}:{phase}"),
            digest: fingerprint(record)?,
            turns: vec![Turn::assistant(format!(
                "{MESSAGE_MARKER}{}",
                to_string(record)?
            ))],
        };
        Ok(self
            .history
            .append_session_in(tx, journal, &request)
            .await?)
    }
}

pub(super) fn fingerprint<T: Serialize>(record: &T) -> MessagingResult<String> {
    Ok(format!("{:x}", Sha256::digest(to_vec(record)?)))
}

/// Decode only a host-marked canonical record. History projection must escape
/// MESSAGE_MARKER in ordinary assistant output before storing it, just as it
/// escapes tool-journal markers. Decoded fields remain attributed message data,
/// never authenticated human/tool authority.
pub fn decode_record(content: &str) -> MessagingResult<Value> {
    let json = content
        .strip_prefix(MESSAGE_MARKER)
        .ok_or(MessagingError::Invalid("missing session message marker"))?;
    Ok(from_str(json)?)
}

#[cfg(test)]
mod tests {
    use octo_history::{CreateSession, SessionStore};
    use sqlx::{query, query_scalar};

    use super::super::tests::{draft, fixed, journal_len, setup, Guard};
    use crate::messaging::{
        contract::{DestinationSpec, MessageIntent, WorkReportPolicy},
        policy::MessagingLimits,
    };

    #[tokio::test]
    async fn admission_policy_is_atomic_idempotent_and_cannot_revert_later_revision() {
        let (store, a, b) = setup(MessagingLimits::default()).await;
        let c = store
            .history
            .create_session(CreateSession::private("alice"))
            .await
            .unwrap();
        let proposal = draft(fixed(&b), MessageIntent::Question);
        let policy = WorkReportPolicy {
            revision: 1,
            intermediate: None,
            final_result: Some(DestinationSpec::FixedSession {
                session: c.id,
                display: None,
            }),
        };
        query("CREATE TRIGGER reject_admission_policy BEFORE INSERT ON albert_message_report_policies BEGIN SELECT RAISE(ABORT,'injected policy failure'); END").execute(store.history.pool()).await.unwrap();
        assert!(store
            .send_with_policy(&a, &proposal, Some(&policy), &Guard, 1)
            .await
            .is_err());
        assert_eq!(journal_len(&store, &a).await, 0);
        let count: i64 = query_scalar("SELECT COUNT(*) FROM albert_messages")
            .fetch_one(store.history.pool())
            .await
            .unwrap();
        assert_eq!(count, 0);
        query("DROP TRIGGER reject_admission_policy")
            .execute(store.history.pool())
            .await
            .unwrap();
        store
            .send_with_policy(&a, &proposal, Some(&policy), &Guard, 2)
            .await
            .unwrap();
        assert_eq!(
            store.message_report_policy(&a, &proposal.id).await.unwrap(),
            Some(policy.clone())
        );
        assert_eq!(
            store
                .status(&a, &proposal.id)
                .await
                .unwrap()
                .intended
                .admission_report_policy,
            Some(policy.clone())
        );
        let mut updated = policy.clone();
        updated.revision = 2;
        updated.final_result = Some(fixed(&a));
        store
            .set_message_report_policy(&a, &proposal.id, &updated, &Guard)
            .await
            .unwrap();
        assert!(
            store
                .send_with_policy(&a, &proposal, Some(&policy), &Guard, 3)
                .await
                .unwrap()
                .duplicate
        );
        assert_eq!(
            store.message_report_policy(&a, &proposal.id).await.unwrap(),
            Some(updated)
        );
        let mut changed = policy.clone();
        changed.final_result = Some(fixed(&b));
        assert!(store
            .send_with_policy(&a, &proposal, Some(&changed), &Guard, 4)
            .await
            .is_err());
        assert_eq!(journal_len(&store, &a).await, 1);
    }
}
