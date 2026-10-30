use octo_history::{PageRequest, SessionId, SessionJournal, SessionStore};
use rand::random;
use serde_json::{from_str, from_value, to_string};
use sqlx::{query, query_as, Sqlite, Transaction};

use super::{
    persistence::{decode_record, MessageRecord},
    MessageStore,
};
use crate::messaging::{
    contract::{ContentRef, MessageEnvelope, MessageId, ResolvedDestination, CONTRACT_VERSION},
    policy::RouteGuard,
    MessagingError, MessagingResult,
};

#[derive(Clone, Debug)]
pub struct DispatchReceipt {
    pub target: ResolvedDestination,
    #[cfg(test)]
    pub recipient: ContentRef,
    #[cfg(test)]
    pub needs_physical_send: bool,
}

/// Token identifies one actual attempt. Stale callbacks cannot overwrite a
/// newer attempt. Started attempts are never implicitly replayed after restart.
#[derive(Clone, Debug)]
pub struct SendAttempt {
    pub token: String,
    pub target: ResolvedDestination,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub enum SendOutcome {
    Delivered,
    KnownNotSent,
}

impl MessageStore {
    /// Resolve channel-active exactly once and append an attributed canonical
    /// recipient copy atomically with its unique (message, recipient) receipt.
    pub async fn dispatch(
        &self,
        id: &MessageId,
        guard: &dyn RouteGuard,
        now: i64,
    ) -> MessagingResult<DispatchReceipt> {
        let _lease = self.lifecycle.read().await;
        let (source, record_id): (String, i64) =
            query_as("SELECT source_session,source_record FROM albert_messages WHERE message_id=?")
                .bind(id.as_str())
                .fetch_optional(self.history.pool())
                .await?
                .ok_or(MessagingError::Unavailable)?;
        let journal = SessionJournal::Root(SessionId::parse(&source)?);
        let page = self
            .history
            .session_page(
                &journal,
                PageRequest {
                    before_id: None,
                    through_id: Some(record_id),
                    limit: 1,
                },
            )
            .await?;
        let record = page
            .records
            .first()
            .filter(|row| row.id == record_id)
            .ok_or(MessagingError::Unavailable)?;
        let mut record: MessageRecord = from_value(decode_record(&record.turn.content)?)?;
        let mut tx = self.history.begin_journal().await?;
        let (envelope_json, resolved, state): (String, Option<String>, String) = query_as(
            "SELECT m.envelope,o.resolved,o.state FROM albert_messages m JOIN albert_message_outbox o USING(message_id) WHERE message_id=?")
            .bind(id.as_str()).fetch_one(&mut *tx).await?;
        if matches!(state.as_str(), "failed" | "unknown" | "started") {
            return Err(MessagingError::Conflict);
        }
        let envelope: MessageEnvelope = from_str(&envelope_json)?;
        self.check_envelope_in(&mut tx, &envelope, now).await?;
        let target = match resolved {
            Some(json) => {
                let target: ResolvedDestination = from_str(&json)?;
                self.recheck_target_in(&mut tx, &envelope.actor, &target, guard)
                    .await?;
                target
            }
            None => {
                self.resolve_in(&mut tx, &envelope.actor, &envelope.intended, guard)
                    .await?
            }
        };
        self.admit_in(&mut tx, &envelope, &target).await?;
        record.record_type = "albert.session_message.received.v1".into();
        record.original = Some(ContentRef { journal, record_id });
        let recipient = self
            .append_in(
                &mut tx,
                &SessionJournal::Root(target.session.clone()),
                id.as_str(),
                "recipient",
                &record,
            )
            .await?;
        query("UPDATE albert_message_inbox SET recipient_record=? WHERE message_id=? AND recipient_session=?")
            .bind(recipient.record_id).bind(id.as_str()).bind(target.session.as_str()).execute(&mut *tx).await?;
        #[cfg(test)]
        let needs_physical_send = target.display.is_some() && state != "delivered";
        query("UPDATE albert_message_outbox SET resolved=?,state=? WHERE message_id=?")
            .bind(to_string(&target)?)
            .bind(if target.display.is_none() {
                "delivered"
            } else {
                &state
            })
            .bind(id.as_str())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(DispatchReceipt {
            target,
            #[cfg(test)]
            recipient,
            #[cfg(test)]
            needs_physical_send,
        })
    }

    /// Commit this before publishing to the connector. The transaction has
    /// ended when this returns; caller must then perform the external publish.
    #[cfg(test)]
    pub async fn send_started(
        &self,
        id: &MessageId,
        guard: &dyn RouteGuard,
        now: i64,
    ) -> MessagingResult<SendAttempt> {
        self.begin_delivery(id, guard, now, "legacy", now.saturating_add(30_000))
            .await
    }

    pub async fn begin_delivery(
        &self,
        id: &MessageId,
        guard: &dyn RouteGuard,
        now: i64,
        transport: &str,
        observation_deadline: i64,
    ) -> MessagingResult<SendAttempt> {
        if !matches!(transport, "telegram" | "console" | "legacy") || observation_deadline <= now {
            return Err(MessagingError::Invalid("invalid delivery attempt"));
        }
        let _lease = self.lifecycle.read().await;
        let mut tx = self.history.begin_journal().await?;
        let (envelope, resolved, state, attempts): (String, Option<String>, String, i64) = query_as(
            "SELECT m.envelope,o.resolved,o.state,o.attempts FROM albert_messages m JOIN albert_message_outbox o USING(message_id) WHERE message_id=?")
            .bind(id.as_str()).fetch_optional(&mut *tx).await?.ok_or(MessagingError::Unavailable)?;
        if !matches!(state.as_str(), "pending" | "retry") || attempts >= self.limits.max_attempts {
            return Err(MessagingError::Conflict);
        }
        let envelope: MessageEnvelope = from_str(&envelope)?;
        self.check_envelope_in(&mut tx, &envelope, now).await?;
        let target: ResolvedDestination = from_str(&resolved.ok_or(MessagingError::Conflict)?)?;
        if target.display.is_none() {
            return Err(MessagingError::Conflict);
        }
        self.recheck_target_in(&mut tx, &envelope.actor, &target, guard)
            .await?;
        let admitted: Option<i64> = query_as::<_, (Option<i64>,)>("SELECT recipient_record FROM albert_message_inbox WHERE message_id=? AND recipient_session=?")
            .bind(id.as_str()).bind(target.session.as_str()).fetch_optional(&mut *tx).await?.and_then(|row| row.0);
        if admitted.is_none() {
            return Err(MessagingError::Conflict);
        }
        let token = format!("{:032x}", random::<u128>());
        query("UPDATE albert_message_outbox SET state='started',attempts=attempts+1,attempt_token=?,failure_code=NULL,transport_kind=?,submitted_at=?,observation_deadline=? WHERE message_id=?")
            .bind(&token).bind(transport).bind(now).bind(observation_deadline).bind(id.as_str()).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(SendAttempt { token, target })
    }

    #[cfg(test)]
    pub async fn send_outcome(
        &self,
        id: &MessageId,
        token: &str,
        outcome: SendOutcome,
    ) -> MessagingResult<()> {
        let mut tx = self.history.begin_journal().await?;
        let (state, code) = match outcome {
            SendOutcome::Delivered => ("delivered", None),
            SendOutcome::KnownNotSent => ("retry", Some("known_not_sent")),
        };
        let changed = query("UPDATE albert_message_outbox SET state=CASE WHEN ?='retry' AND attempts>=? THEN 'failed' ELSE ? END,failure_code=? WHERE message_id=? AND state='started' AND attempt_token=? AND (transport_kind='legacy' OR ?='unknown')")
            .bind(state).bind(self.limits.max_attempts).bind(state).bind(code).bind(id.as_str()).bind(token).bind(state)
            .execute(&mut *tx).await?.rows_affected();
        if changed != 1 {
            return Err(MessagingError::Conflict);
        }
        tx.commit().await?;
        Ok(())
    }

    /// Persist an observable terminal preflight failure (archive/revoke/ended/
    /// expired/unsupported). No free-text payload is stored in the control table.
    pub async fn fail_delivery(
        &self,
        id: &MessageId,
        reason: DeliveryFailure,
    ) -> MessagingResult<()> {
        let mut tx = self.history.begin_journal().await?;
        let changed = query("UPDATE albert_message_outbox SET state='failed',failure_code=? WHERE message_id=? AND state IN ('pending','retry')")
            .bind(reason.code()).bind(id.as_str()).execute(&mut *tx).await?.rows_affected();
        if changed != 1 {
            return Err(MessagingError::Conflict);
        }
        query("UPDATE albert_message_inbox SET work_state='failed' WHERE message_id=? AND work_state='queued' AND recipient_record IS NULL")
            .bind(id.as_str()).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Positive producer-side proof: this attempt has NOT been published.
    /// Never call from an adapter callback or after bus submission.
    pub async fn reject_inactive_before_publish(
        &self,
        id: &MessageId,
        token: Option<&str>,
    ) -> MessagingResult<()> {
        let mut tx = self.history.begin_journal().await?;
        let changed = if let Some(token) = token {
            query("UPDATE albert_message_outbox SET state='failed',failure_code='inactive_view',submitted_at=NULL,observation_deadline=NULL WHERE message_id=? AND state='started' AND attempt_token=? AND NOT EXISTS (SELECT 1 FROM albert_message_delivery_evidence e WHERE e.message_id=albert_message_outbox.message_id)")
                .bind(id.as_str()).bind(token).execute(&mut *tx).await?.rows_affected()
        } else {
            query("UPDATE albert_message_outbox SET state='failed',failure_code='inactive_view' WHERE message_id=? AND state IN ('pending','retry')")
                .bind(id.as_str()).execute(&mut *tx).await?.rows_affected()
        };
        if changed != 1 {
            return Err(MessagingError::Conflict);
        }
        tx.commit().await?;
        Ok(())
    }

    pub(super) async fn check_envelope_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        envelope: &MessageEnvelope,
        now: i64,
    ) -> MessagingResult<()> {
        if envelope.version != CONTRACT_VERSION {
            return Err(MessagingError::Invalid("unsupported message version"));
        }
        if envelope.deadline_unix_ms <= now {
            return Err(MessagingError::Unavailable);
        }
        self.session_in(tx, &envelope.source, &envelope.actor)
            .await?;
        Ok(())
    }

    pub(super) async fn recheck_target_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        actor: &str,
        target: &ResolvedDestination,
        guard: &dyn RouteGuard,
    ) -> MessagingResult<()> {
        self.session_in(tx, &target.session, actor).await?;
        if let Some(route) = &target.display {
            self.verify_route_in(tx, actor, route, guard).await?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub enum DeliveryFailure {
    Unavailable,
    SaveRequired,
    Limit,
    Cancelled,
    Revoked,
    Unsupported,
}
impl DeliveryFailure {
    fn code(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::SaveRequired => "save_required",
            Self::Limit => "limit",
            Self::Cancelled => "cancelled",
            Self::Revoked => "revoked",
            Self::Unsupported => "unsupported",
        }
    }
}
