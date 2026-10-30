use std::collections::HashSet;

use serde::Serialize;
use serde_json::{from_str, to_string};
use sqlx::{query, query_as, query_scalar, Sqlite, Transaction};

use super::{persistence::fingerprint, MessageStore};
use crate::messaging::{
    contract::{MessageId, ResolvedDestination},
    transport::AdapterKind,
    MessagingError, MessagingResult,
};

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryObservation {
    ChunkSent,
    ChunkUnknown,
    ChunkFailed,
    StreamWritten,
    StreamUnknown,
    PublishUnknown,
    ViewSuppressed,
}
impl DeliveryObservation {
    fn code(self) -> &'static str {
        match self {
            Self::ChunkSent => "chunk_sent",
            Self::ChunkUnknown => "chunk_unknown",
            Self::ChunkFailed => "chunk_failed",
            Self::StreamWritten => "stream_written",
            Self::StreamUnknown => "stream_unknown",
            Self::PublishUnknown => "publish_unknown",
            Self::ViewSuppressed => "view_suppressed",
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct DeliveryEvidence {
    pub outcome: String,
    pub native_ids: Vec<String>,
    pub observed_at: i64,
}
#[derive(Clone, Debug, Serialize)]
pub struct PhysicalDelivery {
    pub requested: bool,
    pub adapter: Option<String>,
    pub submitted_at: Option<i64>,
    pub observation_deadline: Option<i64>,
    pub observations: Vec<DeliveryEvidence>,
    pub evidence_truncated: bool,
    pub completion_note: String,
}
pub struct DeliveryEvent<'a> {
    pub token: &'a str,
    pub adapter: AdapterKind,
    pub outcome: DeliveryObservation,
    pub native_ids: &'a [String],
    pub now: i64,
    pub max_observations: usize,
}

impl MessageStore {
    pub async fn observe_delivery(
        &self,
        id: &MessageId,
        event: DeliveryEvent<'_>,
    ) -> MessagingResult<()> {
        let DeliveryEvent {
            token,
            adapter,
            outcome,
            native_ids,
            now,
            max_observations,
        } = event;
        if !(1..=128).contains(&max_observations)
            || native_ids.len() > 16
            || native_ids
                .iter()
                .any(|id| id.len() > 64 || id.contains('\0'))
        {
            return Err(MessagingError::Invalid("invalid delivery evidence"));
        }
        let mut tx = self.history.begin_journal().await?;
        let (stored_token, kind, state): (Option<String>, Option<String>, String) = query_as("SELECT attempt_token,transport_kind,state FROM albert_message_outbox WHERE message_id=?")
            .bind(id.as_str()).fetch_optional(&mut *tx).await?.ok_or(MessagingError::Unavailable)?;
        if stored_token.as_deref() != Some(token)
            || kind.as_deref() != Some(adapter.as_str())
            || !matches!(state.as_str(), "started" | "unknown" | "delivered")
        {
            return Err(MessagingError::Conflict);
        }
        if matches!(
            outcome,
            DeliveryObservation::StreamWritten | DeliveryObservation::StreamUnknown
        ) && adapter != AdapterKind::Console
        {
            return Err(MessagingError::Denied);
        }
        if matches!(
            outcome,
            DeliveryObservation::ChunkSent
                | DeliveryObservation::ChunkUnknown
                | DeliveryObservation::ChunkFailed
        ) && adapter != AdapterKind::Telegram
        {
            return Err(MessagingError::Denied);
        }
        let hash = fingerprint(&(outcome, native_ids))?;
        let exists: i64 = query_scalar("SELECT COUNT(*) FROM albert_message_delivery_evidence WHERE message_id=? AND attempt_token=? AND observation_hash=?")
            .bind(id.as_str()).bind(token).bind(&hash).fetch_one(&mut *tx).await?;
        if exists == 0 {
            let count: i64 = query_scalar(
                "SELECT COUNT(*) FROM albert_message_delivery_evidence WHERE message_id=?",
            )
            .bind(id.as_str())
            .fetch_one(&mut *tx)
            .await?;
            if count < max_observations as i64 {
                query("INSERT INTO albert_message_delivery_evidence VALUES(?,?,?,?,?,?)")
                    .bind(id.as_str())
                    .bind(token)
                    .bind(hash)
                    .bind(outcome.code())
                    .bind(to_string(native_ids)?)
                    .bind(now)
                    .execute(&mut *tx)
                    .await?;
            } else {
                query("UPDATE albert_message_outbox SET evidence_truncated=1 WHERE message_id=?")
                    .bind(id.as_str())
                    .execute(&mut *tx)
                    .await?;
            }
        }
        if matches!(outcome, DeliveryObservation::StreamWritten) {
            query("UPDATE albert_message_outbox SET state='delivered',failure_code=NULL WHERE message_id=? AND attempt_token=?")
                .bind(id.as_str()).bind(token).execute(&mut *tx).await?;
        } else if !matches!(outcome, DeliveryObservation::ChunkSent) {
            query("UPDATE albert_message_outbox SET state='unknown',failure_code='completion_unknown' WHERE message_id=? AND attempt_token=? AND state!='delivered'")
                .bind(id.as_str()).bind(token).execute(&mut *tx).await?;
        }
        // Even every observed Telegram chunk being Sent cannot prove that no
        // further chunk failed/dropped; only a Console full-write hook completes.
        tx.commit().await?;
        Ok(())
    }
    pub async fn expire_delivery_observations(&self, now: i64) -> MessagingResult<u64> {
        Ok(query("UPDATE albert_message_outbox SET state='unknown',failure_code='observation_timeout' WHERE state='started' AND observation_deadline<=?")
            .bind(now).execute(self.history.pool()).await?.rows_affected())
    }
    pub async fn delivery_page(
        &self,
        after: Option<&str>,
        limit: usize,
    ) -> MessagingResult<Vec<MessageId>> {
        if !(1..=128).contains(&limit) {
            return Err(MessagingError::Invalid("invalid delivery page size"));
        }
        let ids: Vec<String> = query_scalar("SELECT o.message_id FROM albert_message_outbox o WHERE o.message_id>? AND o.state IN ('pending','retry') AND o.resolved IS NOT NULL AND EXISTS (SELECT 1 FROM albert_message_inbox i WHERE i.message_id=o.message_id AND i.recipient_record IS NOT NULL) ORDER BY o.message_id LIMIT ?")
            .bind(after.unwrap_or("")).bind(limit as i64).fetch_all(self.history.pool()).await?;
        ids.into_iter()
            .map(|id| {
                MessageId::try_from(id).map_err(|_| MessagingError::Invalid("corrupt message ID"))
            })
            .collect()
    }
    pub async fn resolved_delivery(
        &self,
        id: &MessageId,
    ) -> MessagingResult<(String, ResolvedDestination)> {
        let (actor, target): (String, String) = query_as("SELECT m.actor_id,o.resolved FROM albert_messages m JOIN albert_message_outbox o USING(message_id) WHERE message_id=?")
            .bind(id.as_str()).fetch_optional(self.history.pool()).await?.ok_or(MessagingError::Unavailable)?;
        Ok((actor, from_str(&target)?))
    }
    pub(super) async fn physical_delivery_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        id: &MessageId,
        requested: bool,
    ) -> MessagingResult<PhysicalDelivery> {
        let (adapter, submitted_at, deadline, truncated, state): (Option<String>, Option<i64>, Option<i64>, i64, String) = query_as("SELECT transport_kind,submitted_at,observation_deadline,evidence_truncated,state FROM albert_message_outbox WHERE message_id=?")
            .bind(id.as_str()).fetch_one(&mut **tx).await?;
        let rows: Vec<(String, String, i64)> = query_as("SELECT outcome,native_ids,observed_at FROM albert_message_delivery_evidence WHERE message_id=? ORDER BY observed_at,observation_hash")
            .bind(id.as_str()).fetch_all(&mut **tx).await?;
        let observations = rows
            .into_iter()
            .map(|(outcome, ids, observed_at)| {
                Ok(DeliveryEvidence {
                    outcome,
                    native_ids: from_str(&ids)?,
                    observed_at,
                })
            })
            .collect::<MessagingResult<Vec<_>>>()?;
        let inactive: Option<String> =
            query_scalar("SELECT failure_code FROM albert_message_outbox WHERE message_id=?")
                .bind(id.as_str())
                .fetch_one(&mut **tx)
                .await?;
        let note = if inactive.as_deref() == Some("inactive_view") {
            "Not sent: recipient Session is inactive on this private route. Canonical admission retained; no automatic resend on open.".into()
        } else if !requested {
            "No physical display was requested; delivery state describes canonical recipient admission only.".into()
        } else if adapter.as_deref() == Some("telegram") {
            let chunks = observations
                .iter()
                .filter(|item| item.outcome == "chunk_sent")
                .flat_map(|item| item.native_ids.iter())
                .collect::<HashSet<_>>()
                .len();
            format!("Observed {chunks} sent native message IDs; whole-report completion is unobservable with this adapter. Submission is {state}; no blind resend after uncertainty.")
        } else if state == "delivered" && adapter.as_deref() == Some("console") {
            "Console write and flush confirmed; human reading is not confirmed.".into()
        } else if adapter.as_deref() == Some("console") && state == "unknown" {
            "Console output is uncertain; write or flush may have partially succeeded. No automatic resend; human reading is not confirmed.".into()
        } else {
            "Canonical receipt, physical submission and completion are separate facts.".into()
        };
        Ok(PhysicalDelivery {
            requested,
            adapter,
            submitted_at,
            observation_deadline: deadline,
            observations,
            evidence_truncated: truncated != 0,
            completion_note: note,
        })
    }
}

#[cfg(test)]
mod tests {
    use sqlx::{query, query_scalar};

    use super::super::{
        tests::{display, draft, fixed, journal_len, setup, Guard},
        DeliveryState, SendOutcome, WorkState, WorkTerminal,
    };
    use super::{DeliveryEvent, DeliveryObservation};
    use crate::messaging::{
        contract::{DestinationSpec, MessageIntent},
        policy::MessagingLimits,
        transport::AdapterKind,
    };

    #[tokio::test]
    async fn telegram_chunk_success_unknown_and_late_evidence_never_complete_report() {
        let (store, a, _) = setup(MessagingLimits::default()).await;
        let proposal = draft(
            DestinationSpec::ChannelActive { route: display() },
            MessageIntent::Notice,
        );
        store.send(&a, &proposal, &Guard, 1).await.unwrap();
        store.dispatch(&proposal.id, &Guard, 2).await.unwrap();
        let attempt = store
            .begin_delivery(&proposal.id, &Guard, 3, "telegram", 10)
            .await
            .unwrap();
        store
            .observe_delivery(
                &proposal.id,
                DeliveryEvent {
                    token: &attempt.token,
                    adapter: AdapterKind::Telegram,
                    outcome: DeliveryObservation::ChunkSent,
                    native_ids: &["7:1".into()],
                    now: 4,
                    max_observations: 8,
                },
            )
            .await
            .unwrap();
        assert_eq!(
            store.status(&a, &proposal.id).await.unwrap().delivery,
            DeliveryState::Started
        );
        assert!(store
            .send_outcome(&proposal.id, &attempt.token, SendOutcome::Delivered)
            .await
            .is_err());
        store
            .observe_delivery(
                &proposal.id,
                DeliveryEvent {
                    token: &attempt.token,
                    adapter: AdapterKind::Telegram,
                    outcome: DeliveryObservation::ChunkUnknown,
                    native_ids: &[],
                    now: 5,
                    max_observations: 8,
                },
            )
            .await
            .unwrap();
        store
            .observe_delivery(
                &proposal.id,
                DeliveryEvent {
                    token: &attempt.token,
                    adapter: AdapterKind::Telegram,
                    outcome: DeliveryObservation::ChunkSent,
                    native_ids: &["7:2".into()],
                    now: 20,
                    max_observations: 8,
                },
            )
            .await
            .unwrap();
        let status = store.status(&a, &proposal.id).await.unwrap();
        assert_eq!(status.delivery, DeliveryState::Unknown);
        assert_eq!(status.physical.observations.len(), 3);
        assert!(status
            .physical
            .completion_note
            .contains("2 sent native message IDs"));
        assert!(store.delivery_page(None, 16).await.unwrap().is_empty());
        assert!(store
            .observe_delivery(
                &proposal.id,
                DeliveryEvent {
                    token: "stale-token",
                    adapter: AdapterKind::Telegram,
                    outcome: DeliveryObservation::ChunkSent,
                    native_ids: &["7:3".into()],
                    now: 21,
                    max_observations: 8
                }
            )
            .await
            .is_err());
        assert_eq!(
            store
                .status(&a, &proposal.id)
                .await
                .unwrap()
                .physical
                .observations
                .len(),
            3
        );
    }

    #[tokio::test]
    async fn silent_drop_timeout_and_restart_are_unknown_without_resend() {
        let (store, a, _) = setup(MessagingLimits::default()).await;
        let first = draft(
            DestinationSpec::ChannelActive { route: display() },
            MessageIntent::Notice,
        );
        store.send(&a, &first, &Guard, 1).await.unwrap();
        store.dispatch(&first.id, &Guard, 2).await.unwrap();
        let attempt = store
            .begin_delivery(&first.id, &Guard, 3, "telegram", 10)
            .await
            .unwrap();
        assert_eq!(store.expire_delivery_observations(11).await.unwrap(), 1);
        let status = store.status(&a, &first.id).await.unwrap();
        assert_eq!(status.delivery, DeliveryState::Unknown);
        assert!(status.physical.observations.is_empty());
        store
            .observe_delivery(
                &first.id,
                DeliveryEvent {
                    token: &attempt.token,
                    adapter: AdapterKind::Telegram,
                    outcome: DeliveryObservation::ChunkSent,
                    native_ids: &["7:9".into()],
                    now: 12,
                    max_observations: 8,
                },
            )
            .await
            .unwrap();
        assert_eq!(
            store.status(&a, &first.id).await.unwrap().delivery,
            DeliveryState::Unknown
        );
        let second = draft(
            DestinationSpec::ChannelActive { route: display() },
            MessageIntent::Notice,
        );
        store.send(&a, &second, &Guard, 2).await.unwrap();
        store.dispatch(&second.id, &Guard, 3).await.unwrap();
        store
            .begin_delivery(&second.id, &Guard, 4, "telegram", 100)
            .await
            .unwrap();
        store.recover(16).await.unwrap();
        assert_eq!(
            store.status(&a, &second.id).await.unwrap().delivery,
            DeliveryState::Unknown
        );
        assert!(store.delivery_page(None, 16).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn uncertainty_releases_active_queue_capacity_without_erasing_receipts() {
        let limits = MessagingLimits {
            max_outbox_pending: 1,
            ..MessagingLimits::default()
        };
        let (store, a, _) = setup(limits).await;
        let first = draft(
            DestinationSpec::ChannelActive { route: display() },
            MessageIntent::Notice,
        );
        store.send(&a, &first, &Guard, 1).await.unwrap();
        store.dispatch(&first.id, &Guard, 2).await.unwrap();
        store
            .begin_delivery(&first.id, &Guard, 3, "telegram", 10)
            .await
            .unwrap();
        store.expire_delivery_observations(11).await.unwrap();
        let second = draft(
            DestinationSpec::ChannelActive { route: display() },
            MessageIntent::Notice,
        );
        store.send(&a, &second, &Guard, 12).await.unwrap();
        assert!(store
            .send(
                &a,
                &draft(
                    DestinationSpec::ChannelActive { route: display() },
                    MessageIntent::Notice
                ),
                &Guard,
                13
            )
            .await
            .is_err());
        assert_eq!(
            store.status(&a, &first.id).await.unwrap().delivery,
            DeliveryState::Unknown
        );

        let limits = MessagingLimits {
            max_inbox_pending: 1,
            ..MessagingLimits::default()
        };
        let (store, a, b) = setup(limits).await;
        let work = draft(fixed(&b), MessageIntent::Question);
        store.send(&a, &work, &Guard, 1).await.unwrap();
        store.dispatch(&work.id, &Guard, 2).await.unwrap();
        store.claim_work(&b, &work.id, &Guard, 3).await.unwrap();
        store
            .finish_work(&b, &work.id, WorkTerminal::Interrupted)
            .await
            .unwrap();
        store
            .send(&a, &draft(fixed(&b), MessageIntent::Question), &Guard, 4)
            .await
            .unwrap();
        assert_eq!(
            store.status(&a, &work.id).await.unwrap().work,
            Some(WorkState::Unknown)
        );
    }

    #[tokio::test]
    async fn bounded_uncertainty_expiry_audits_evidence_and_preserves_canonical_idempotency() {
        let (store, a, b) = setup(MessagingLimits::default()).await;
        let proposal = draft(
            DestinationSpec::ChannelActive { route: display() },
            MessageIntent::Notice,
        );
        store.send(&a, &proposal, &Guard, 1).await.unwrap();
        store.dispatch(&proposal.id, &Guard, 2).await.unwrap();
        let attempt = store
            .begin_delivery(&proposal.id, &Guard, 3, "telegram", 10)
            .await
            .unwrap();
        store
            .observe_delivery(
                &proposal.id,
                DeliveryEvent {
                    token: &attempt.token,
                    adapter: AdapterKind::Telegram,
                    outcome: DeliveryObservation::ChunkUnknown,
                    native_ids: &[],
                    now: 4,
                    max_observations: 8,
                },
            )
            .await
            .unwrap();
        assert_eq!(
            store.cleanup_expired_unknown(1100, 100, 16).await.unwrap(),
            0
        );
        assert_eq!(
            store.cleanup_expired_unknown(1101, 100, 16).await.unwrap(),
            1
        );
        assert_eq!(journal_len(&store, &a).await, 2);
        assert_eq!(journal_len(&store, &b).await, 1);
        assert!(store.status(&a, &proposal.id).await.is_err());
        let count: i64 = query_scalar("SELECT COUNT(*) FROM albert_message_delivery_evidence")
            .fetch_one(store.history.pool())
            .await
            .unwrap();
        assert_eq!(count, 0);
        let mut changed = proposal.clone();
        changed.deadline_unix_ms = 5000;
        assert!(store.send(&a, &changed, &Guard, 1102).await.is_err());
        let count: i64 = query_scalar("SELECT COUNT(*) FROM albert_messages")
            .fetch_one(store.history.pool())
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn evidence_is_bounded_deduplicated_and_stale_attempts_cannot_complete() {
        let (store, a, _) = setup(MessagingLimits::default()).await;
        let proposal = draft(
            DestinationSpec::ChannelActive { route: display() },
            MessageIntent::Notice,
        );
        store.send(&a, &proposal, &Guard, 1).await.unwrap();
        store.dispatch(&proposal.id, &Guard, 2).await.unwrap();
        let first = store
            .begin_delivery(&proposal.id, &Guard, 3, "telegram", 10)
            .await
            .unwrap();
        for id in ["7:1", "7:1", "7:2", "7:3"] {
            store
                .observe_delivery(
                    &proposal.id,
                    DeliveryEvent {
                        token: &first.token,
                        adapter: AdapterKind::Telegram,
                        outcome: DeliveryObservation::ChunkSent,
                        native_ids: &[id.into()],
                        now: 4,
                        max_observations: 2,
                    },
                )
                .await
                .unwrap();
        }
        let status = store.status(&a, &proposal.id).await.unwrap();
        assert_eq!(status.physical.observations.len(), 2);
        assert!(status.physical.evidence_truncated);
        assert!(store
            .observe_delivery(
                &proposal.id,
                DeliveryEvent {
                    token: &first.token,
                    adapter: AdapterKind::Telegram,
                    outcome: DeliveryObservation::StreamWritten,
                    native_ids: &[],
                    now: 5,
                    max_observations: 2
                }
            )
            .await
            .is_err());
        query("UPDATE albert_message_outbox SET state='pending' WHERE message_id=?")
            .bind(proposal.id.as_str())
            .execute(store.history.pool())
            .await
            .unwrap();
        let next = store
            .begin_delivery(&proposal.id, &Guard, 6, "telegram", 20)
            .await
            .unwrap();
        assert_ne!(first.token, next.token);
        assert!(store
            .observe_delivery(
                &proposal.id,
                DeliveryEvent {
                    token: &first.token,
                    adapter: AdapterKind::Telegram,
                    outcome: DeliveryObservation::ChunkUnknown,
                    native_ids: &[],
                    now: 7,
                    max_observations: 2
                }
            )
            .await
            .is_err());
        assert_eq!(
            store.status(&a, &proposal.id).await.unwrap().delivery,
            DeliveryState::Started
        );
    }
}
