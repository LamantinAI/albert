use crate::messaging::store::{MessageStore, RecoveryItem};
use crate::{messaging::MessagingError, messaging::MessagingResult};
use sqlx::{query, query_scalar};
impl MessageStore {
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
