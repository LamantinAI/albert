use octo_history::{SessionAccess, SessionId, SessionJournal};
use serde::Serialize;
use serde_json::{from_str, Value};
use sqlx::{query_as, query_scalar, Sqlite, Transaction};

use super::{evidence::PhysicalDelivery, MessageStore, RecoveryAction, RecoveryItem, WorkState};
use crate::messaging::{
    contract::{
        ContentRef, DestinationSpec, MessageEnvelope, MessageId, ResolvedDestination, TrustedSource,
    },
    MessagingError, MessagingResult,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryState {
    Pending,
    Started,
    Delivered,
    Retry,
    Unknown,
    Failed,
}

#[derive(Clone, Debug, Serialize)]
pub struct MessageStatus {
    pub accepted: ContentRef,
    pub intended: MessageEnvelope,
    pub resolved: Option<ResolvedDestination>,
    pub received: Option<ContentRef>,
    pub result: Option<ContentRef>,
    pub work: Option<WorkState>,
    pub work_failure_code: Option<String>,
    /// Delivered with no display route means canonical admission only. With a
    /// display route it means a positive physical send callback was committed.
    pub delivery: DeliveryState,
    pub physical: PhysicalDelivery,
    pub report_outcomes: Option<Value>,
    pub attempts: i64,
    pub failure_code: Option<String>,
}

type MessageStatusRow = (
    String,
    i64,
    String,
    Option<String>,
    String,
    i64,
    Option<String>,
);
type InboxStatusRow = (String, Option<i64>, Option<i64>, String, Option<String>);

impl MessageStore {
    pub async fn status(
        &self,
        source: &TrustedSource,
        id: &MessageId,
    ) -> MessagingResult<MessageStatus> {
        if !source.context.is_live() {
            return Err(MessagingError::Unavailable);
        }
        let _lease = self.lifecycle.read().await;
        let mut tx = self.history.begin_journal().await?;
        let actor = source.context.private_user_id()?.as_str();
        self.session_in(&mut tx, source.context.session_id(), actor)
            .await?;
        let row: Option<MessageStatusRow> = query_as(
            "SELECT m.source_session,m.source_record,m.envelope,o.resolved,o.state,o.attempts,o.failure_code FROM albert_messages m JOIN albert_message_outbox o USING(message_id) WHERE message_id=? AND actor_id=?")
            .bind(id.as_str()).bind(actor).fetch_optional(&mut *tx).await?;
        let (session, record_id, envelope, resolved, delivery, attempts, failure_code) =
            row.ok_or(MessagingError::Unavailable)?;
        let session = SessionId::parse(&session)?;
        self.status_owner_in(&mut tx, &session, actor).await?;
        let inbox: Option<InboxStatusRow> = query_as("SELECT recipient_session,recipient_record,result_record,work_state,failure_code FROM albert_message_inbox WHERE message_id=?")
            .bind(id.as_str()).fetch_optional(&mut *tx).await?;
        let (received, result, work, work_failure_code) =
            if let Some((recipient, received, result, state, failure_code)) = inbox {
                let recipient = SessionId::parse(&recipient)?;
                self.status_owner_in(&mut tx, &recipient, actor).await?;
                let journal = SessionJournal::Root(recipient);
                (
                    received.map(|record_id| ContentRef {
                        journal: journal.clone(),
                        record_id,
                    }),
                    result.map(|record_id| ContentRef { journal, record_id }),
                    Some(work_state(&state)?),
                    failure_code,
                )
            } else {
                (None, None, None, None)
            };
        let delivery = match delivery.as_str() {
            "pending" => DeliveryState::Pending,
            "started" => DeliveryState::Started,
            "delivered" => DeliveryState::Delivered,
            "retry" => DeliveryState::Retry,
            "unknown" => DeliveryState::Unknown,
            "failed" => DeliveryState::Failed,
            _ => return Err(MessagingError::Conflict),
        };
        let intended: MessageEnvelope = from_str(&envelope)?;
        let physical_requested = !matches!(
            &intended.intended,
            DestinationSpec::FixedSession { display: None, .. }
        );
        let physical = self
            .physical_delivery_in(&mut tx, id, physical_requested)
            .await?;
        let outcomes: Option<String> =
            query_scalar("SELECT report_outcomes FROM albert_message_inbox WHERE message_id=?")
                .bind(id.as_str())
                .fetch_optional(&mut *tx)
                .await?
                .flatten();
        let status = MessageStatus {
            accepted: ContentRef {
                journal: SessionJournal::Root(session),
                record_id,
            },
            intended,
            resolved: resolved.map(|json| from_str(&json)).transpose()?,
            received,
            result,
            work,
            work_failure_code,
            delivery,
            physical,
            report_outcomes: outcomes.map(|value| from_str(&value)).transpose()?,
            attempts,
            failure_code,
        };
        tx.commit().await?;
        Ok(status)
    }

    // Terminal delivery metadata remains inspectable by its canonical owner
    // even after a source/recipient is archived or ended. This grants no journal
    // content, tool execution, route authorization or renewed session lease.
    async fn status_owner_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        id: &SessionId,
        actor: &str,
    ) -> MessagingResult<()> {
        let session = self
            .history
            .get_session_in(tx, id)
            .await?
            .ok_or(MessagingError::Unavailable)?;
        match session.access {
            SessionAccess::Private { owner } if owner == actor => Ok(()),
            _ => Err(MessagingError::Denied),
        }
    }

    /// Local scheduler discovery only. claim_work performs fresh authorization
    /// and lifecycle/capability checks before any model/tools may run.
    #[cfg(test)]
    pub async fn queued_work(
        &self,
        source: &TrustedSource,
        limit: usize,
    ) -> MessagingResult<Vec<String>> {
        if !(1..=1000).contains(&limit) {
            return Err(MessagingError::Invalid("invalid queue page size"));
        }
        if !source.context.is_live() {
            return Err(MessagingError::Unavailable);
        }
        let _lease = self.lifecycle.read().await;
        let mut tx = self.history.begin_journal().await?;
        self.session_in(
            &mut tx,
            source.context.session_id(),
            source.context.private_user_id()?.as_str(),
        )
        .await?;
        let result = query_scalar("SELECT message_id FROM albert_message_inbox WHERE recipient_session=? AND work_state='queued' AND recipient_record IS NOT NULL ORDER BY message_id LIMIT ?")
            .bind(source.context.session_id().as_str()).bind(limit as i64).fetch_all(&mut *tx).await?;
        tx.commit().await?;
        Ok(result)
    }

    /// Startup/operator continuation after recover(). Opaque keyset cursor avoids
    /// starving records beyond the first bounded page. Not a user discovery API.
    pub async fn recovery_page(
        &self,
        after: Option<&str>,
        limit: usize,
    ) -> MessagingResult<Vec<RecoveryItem>> {
        if !(1..=1000).contains(&limit) {
            return Err(MessagingError::Invalid("invalid recovery page size"));
        }
        let rows: Vec<(String, String, Option<String>)> = query_as(
            "SELECT o.message_id,o.state,i.work_state FROM albert_message_outbox o LEFT JOIN albert_message_inbox i USING(message_id) WHERE o.message_id>? ORDER BY o.message_id LIMIT ?")
            .bind(after.unwrap_or("")).bind(limit as i64).fetch_all(self.history.pool()).await?;
        let mut result = Vec::new();
        for (id, delivery, work) in rows {
            let work_action = match work.as_deref() {
                Some("queued") => Some(RecoveryAction::RecheckQueued),
                Some("unknown") => Some(RecoveryAction::InterruptedUnknown),
                _ => None,
            };
            if let Some(action) = work_action {
                result.push(RecoveryItem {
                    message_id: id.clone(),
                    action,
                });
            }
            let action = match delivery.as_str() {
                "pending" => RecoveryAction::ContinueDelivery,
                "retry" => RecoveryAction::RetryKnownFailure,
                "unknown" => RecoveryAction::TransportUnknown,
                _ => RecoveryAction::Terminal,
            };
            result.push(RecoveryItem {
                message_id: id,
                action,
            });
        }
        Ok(result)
    }
}

fn work_state(value: &str) -> MessagingResult<WorkState> {
    match value {
        "passive" => Ok(WorkState::Passive),
        "queued" => Ok(WorkState::Queued),
        "running" => Ok(WorkState::Running),
        "completed" => Ok(WorkState::Completed),
        "failed" => Ok(WorkState::Failed),
        "cancelled" => Ok(WorkState::Cancelled),
        "unknown" => Ok(WorkState::Unknown),
        _ => Err(MessagingError::Conflict),
    }
}
