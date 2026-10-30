//! Host scheduler enumeration, never a model-facing cross-user catalog.
use octo_history::SessionId;
use serde_json::from_str;
use sqlx::{query, query_as};

use super::MessageStore;
use crate::messaging::{
    contract::{MessageEnvelope, MessageId},
    MessagingError, MessagingResult,
};

#[derive(Clone, Debug)]
pub struct QueueEntry {
    pub envelope: MessageEnvelope,
    pub recipient: Option<SessionId>,
    pub has_copy: bool,
}
impl MessageStore {
    pub async fn scheduler_page(
        &self,
        after: Option<&str>,
        limit: usize,
        now: i64,
    ) -> MessagingResult<Vec<QueueEntry>> {
        if !(1..=128).contains(&limit) {
            return Err(MessagingError::Invalid("invalid scheduler page size"));
        }
        let mut tx = self.history.begin_journal().await?;
        query("UPDATE albert_message_inbox SET work_state='failed',failure_code='expired' WHERE work_state='queued' AND message_id IN (SELECT message_id FROM albert_messages WHERE deadline_unix_ms<=?)")
            .bind(now).execute(&mut *tx).await?;
        query("UPDATE albert_message_outbox SET state='failed',failure_code='expired' WHERE state IN ('pending','retry') AND message_id IN (SELECT message_id FROM albert_messages WHERE deadline_unix_ms<=?)")
            .bind(now).execute(&mut *tx).await?;
        let rows: Vec<(String, Option<String>, Option<i64>)> = query_as(
            "SELECT m.envelope,i.recipient_session,i.recipient_record FROM albert_messages m JOIN albert_message_outbox o USING(message_id) LEFT JOIN albert_message_inbox i USING(message_id) WHERE m.message_id>? AND m.deadline_unix_ms>? AND ((o.state='pending' AND (i.recipient_record IS NULL)) OR (i.work_state='queued' AND i.recipient_record IS NOT NULL)) ORDER BY m.message_id LIMIT ?")
            .bind(after.unwrap_or("")).bind(now).bind(limit as i64).fetch_all(&mut *tx).await?;
        let mut result = Vec::new();
        for (envelope, recipient, record) in rows {
            result.push(QueueEntry {
                envelope: from_str(&envelope)?,
                recipient: recipient.map(|id| SessionId::parse(&id)).transpose()?,
                has_copy: record.is_some(),
            });
        }
        tx.commit().await?;
        Ok(result)
    }
}

impl MessageStore {
    pub async fn failure_notices(
        &self,
        limit: usize,
    ) -> MessagingResult<Vec<(MessageEnvelope, String)>> {
        if !(1..=128).contains(&limit) {
            return Err(MessagingError::Invalid("invalid failure notice page size"));
        }
        let rows: Vec<(String, Option<String>)> = query_as("SELECT m.envelope,COALESCE(i.failure_code,o.failure_code) FROM albert_messages m JOIN albert_message_outbox o USING(message_id) LEFT JOIN albert_message_inbox i USING(message_id) WHERE o.failure_notified=0 AND (i.work_state IN ('failed','cancelled','unknown') OR (o.state='failed' AND COALESCE(i.work_state,'passive') NOT IN ('queued','running','completed'))) ORDER BY m.created_at LIMIT ?")
            .bind(limit as i64).fetch_all(self.history.pool()).await?;
        rows.into_iter()
            .map(|(envelope, code)| {
                Ok((
                    from_str(&envelope)?,
                    code.unwrap_or_else(|| "unknown".into()),
                ))
            })
            .collect()
    }
    pub async fn acknowledge_failure_notice(&self, id: &MessageId) -> MessagingResult<()> {
        let mut tx = self.history.begin_journal().await?;
        query("UPDATE albert_message_outbox SET failure_notified=1 WHERE message_id=?")
            .bind(id.as_str())
            .execute(&mut *tx)
            .await?;
        query("DELETE FROM albert_message_report_policies WHERE message_id=?")
            .bind(id.as_str())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
}
