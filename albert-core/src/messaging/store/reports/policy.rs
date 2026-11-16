use crate::messaging::store::MessageStore;
use crate::{
    messaging::contract::MessageEnvelope, messaging::contract::MessageId,
    messaging::contract::TrustedSource, messaging::contract::WorkReportPolicy,
    messaging::policy::RouteGuard, messaging::MessagingError, messaging::MessagingResult,
};
use serde_json::{from_str, to_string};
use sqlx::{query, query_as, Sqlite, Transaction};

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
    pub(in crate::messaging::store) async fn set_message_report_policy_in(
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
