use octo_history::{SessionId, SessionJournal};
use serde_json::{from_str, Value};
use sqlx::{query, query_as, Sqlite, Transaction};

use super::MessageStore;
use crate::messaging::{MessagingError, MessagingResult};

type UncertainControlRow = (String, String, i64, Option<String>, String, Option<String>);

impl MessageStore {
    /// Unknown work/transport never becomes retryable. Once the original
    /// deadline and configured retention expire, archive uncertainty/evidence in
    /// the canonical source journal and retire only bounded control metadata.
    pub async fn cleanup_expired_unknown(
        &self,
        now: i64,
        retention_ms: i64,
        limit: usize,
    ) -> MessagingResult<u64> {
        if retention_ms < 0 || !(1..=128).contains(&limit) {
            return Err(MessagingError::Invalid("invalid uncertainty retention"));
        }
        let _lease = self.lifecycle.read().await;
        let mut tx = self.history.begin_journal().await?;
        let rows: Vec<UncertainControlRow> = query_as("SELECT m.message_id,m.source_session,m.source_record,o.resolved,o.state,i.work_state FROM albert_messages m JOIN albert_message_outbox o USING(message_id) LEFT JOIN albert_message_inbox i USING(message_id) WHERE m.deadline_unix_ms<? AND (o.state='unknown' OR i.work_state='unknown') AND COALESCE(i.work_state,'passive') NOT IN ('queued','running') ORDER BY m.deadline_unix_ms LIMIT ?")
            .bind(now.saturating_sub(retention_ms)).bind(limit as i64).fetch_all(&mut *tx).await?;
        for (id, source, record, resolved, delivery, work) in &rows {
            let evidence: Vec<(String, String)> = query_as("SELECT outcome,native_ids FROM albert_message_delivery_evidence WHERE message_id=? ORDER BY observed_at,observation_hash")
                .bind(id).fetch_all(&mut *tx).await?;
            let native: Vec<Value> = evidence
                .into_iter()
                .map(|(outcome, ids)| {
                    Ok(Value::Array(vec![Value::String(outcome), from_str(&ids)?]))
                })
                .collect::<MessagingResult<_>>()?;
            let audit = (
                "albert.delivery.expired.v1",
                id,
                record,
                delivery,
                work,
                resolved,
                native,
            );
            self.append_in(
                &mut tx,
                &SessionJournal::Root(SessionId::parse(source)?),
                id,
                "uncertainty_expired",
                &audit,
            )
            .await?;
            self.delete_control_in(&mut tx, id).await?;
        }
        tx.commit().await?;
        Ok(rows.len() as u64)
    }
    pub(super) async fn delete_control_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        id: &str,
    ) -> MessagingResult<()> {
        for table in [
            "albert_message_delivery_evidence",
            "albert_message_report_policies",
            "albert_message_inbox",
            "albert_message_outbox",
            "albert_messages",
        ] {
            // Closed, host-authored identifier list, never input interpolation.
            let sql = format!("DELETE FROM {table} WHERE message_id=?");
            query(&sql).bind(id).execute(&mut **tx).await?;
        }
        Ok(())
    }
}
