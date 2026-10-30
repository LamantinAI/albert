use serde_json::{from_str, to_string};
use sqlx::{query, query_as, query_scalar, Sqlite, Transaction};

use super::MessageStore;
use crate::messaging::{
    contract::{ExecutionLocation, TrustedSource, WorkRef, WorkReportPolicy},
    policy::{destination, identifier, RouteGuard},
    MessagingError, MessagingResult,
};

impl MessageStore {
    pub async fn execution_location(
        &self,
        source: &TrustedSource,
    ) -> MessagingResult<(ExecutionLocation, i64)> {
        let _lease = self.lifecycle.read().await;
        let mut tx = self.history.begin_journal().await?;
        self.metadata_source_in(&mut tx, source).await?;
        let row: Option<(String, i64)> =
            query_as("SELECT location,revision FROM albert_session_execution WHERE session_id=?")
                .bind(source.context.session_id().as_str())
                .fetch_optional(&mut *tx)
                .await?;
        let result = match row {
            Some((json, revision)) => (from_str(&json)?, revision),
            None => (ExecutionLocation::default(), 0),
        };
        tx.commit().await?;
        Ok(result)
    }

    pub async fn set_execution_location(
        &self,
        source: &TrustedSource,
        location: &ExecutionLocation,
        expected_revision: i64,
    ) -> MessagingResult<i64> {
        if let Some(connector) = &location.connector {
            identifier(connector)?;
        }
        if expected_revision < 0 {
            return Err(MessagingError::Invalid("invalid revision"));
        }
        let _lease = self.lifecycle.read().await;
        let mut tx = self.history.begin_journal().await?;
        self.metadata_source_in(&mut tx, source).await?;
        let changed = if expected_revision == 0 {
            query("INSERT OR IGNORE INTO albert_session_execution(session_id,location,revision) VALUES(?,?,1)")
                .bind(source.context.session_id().as_str()).bind(to_string(location)?)
                .execute(&mut *tx).await?.rows_affected()
        } else {
            query("UPDATE albert_session_execution SET location=?,revision=revision+1 WHERE session_id=? AND revision=?")
                .bind(to_string(location)?).bind(source.context.session_id().as_str()).bind(expected_revision)
                .execute(&mut *tx).await?.rows_affected()
        };
        if changed != 1 {
            return Err(MessagingError::Conflict);
        }
        tx.commit().await?;
        Ok(expected_revision + 1)
    }

    pub async fn report_policy(
        &self,
        source: &TrustedSource,
    ) -> MessagingResult<Option<WorkReportPolicy>> {
        let _lease = self.lifecycle.read().await;
        let mut tx = self.history.begin_journal().await?;
        self.metadata_source_in(&mut tx, source).await?;
        let row: Option<(String,)> =
            query_as("SELECT policy FROM albert_work_reports WHERE session_id=? AND work_ref=?")
                .bind(source.context.session_id().as_str())
                .bind(to_string(&source.work)?)
                .fetch_optional(&mut *tx)
                .await?;
        let result = row.map(|(json,)| from_str(&json)).transpose()?;
        tx.commit().await?;
        Ok(result)
    }

    /// Destinations are validated now, but channel-active resolution is not
    /// persisted in configuration. Each actual report resolves at dispatch.
    pub async fn set_report_policy(
        &self,
        source: &TrustedSource,
        policy: &WorkReportPolicy,
        guard: &dyn RouteGuard,
    ) -> MessagingResult<()> {
        if policy.revision < 1 || to_string(policy)?.len() > self.limits.max_metadata_bytes {
            return Err(MessagingError::Invalid("invalid report policy"));
        }
        let _lease = self.lifecycle.read().await;
        let mut tx = self.history.begin_journal().await?;
        self.metadata_source_in(&mut tx, source).await?;
        let actor = source.context.private_user_id()?.as_str();
        for target in [&policy.intermediate, &policy.final_result]
            .into_iter()
            .flatten()
        {
            destination(target)?;
            self.resolve_in(&mut tx, actor, target, guard).await?;
        }
        let changed = if policy.revision == 1 {
            let count: i64 =
                query_scalar("SELECT COUNT(*) FROM albert_work_reports WHERE session_id=?")
                    .bind(source.context.session_id().as_str())
                    .fetch_one(&mut *tx)
                    .await?;
            if count >= self.limits.max_work_policies {
                return Err(MessagingError::Limit);
            }
            query("INSERT OR IGNORE INTO albert_work_reports(session_id,work_ref,policy,revision) VALUES(?,?,?,1)")
                .bind(source.context.session_id().as_str()).bind(to_string(&source.work)?).bind(to_string(policy)?)
                .execute(&mut *tx).await?.rows_affected()
        } else {
            query("UPDATE albert_work_reports SET policy=?,revision=? WHERE session_id=? AND work_ref=? AND revision=?")
                .bind(to_string(policy)?).bind(policy.revision).bind(source.context.session_id().as_str()).bind(to_string(&source.work)?).bind(policy.revision-1)
                .execute(&mut *tx).await?.rows_affected()
        };
        if changed != 1 {
            return Err(MessagingError::Conflict);
        }
        tx.commit().await?;
        Ok(())
    }

    /// Host calls after a work scope is terminal; defaults on other work are unaffected.
    pub async fn clear_report_policy(&self, source: &TrustedSource) -> MessagingResult<()> {
        let _lease = self.lifecycle.read().await;
        let mut tx = self.history.begin_journal().await?;
        self.metadata_source_in(&mut tx, source).await?;
        query("DELETE FROM albert_work_reports WHERE session_id=? AND work_ref=?")
            .bind(source.context.session_id().as_str())
            .bind(to_string(&source.work)?)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn metadata_source_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        source: &TrustedSource,
    ) -> MessagingResult<()> {
        if !source.context.is_live() {
            return Err(MessagingError::Unavailable);
        }
        let actor = source.context.private_user_id()?.as_str();
        self.session_in(tx, source.context.session_id(), actor)
            .await?;
        match &source.work {
            WorkRef::Turn(id) | WorkRef::Child(id) => identifier(id)?,
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{from_value, to_value};

    use super::super::tests::{display, draft, fixed, messages, setup, Guard};
    use crate::messaging::{
        contract::{
            DestinationSpec, ExecutionLocation, MessageDraft, MessageIntent, WorkReportPolicy,
        },
        policy::MessagingLimits,
    };
    #[tokio::test]
    async fn execution_defaults_and_work_report_policy_use_independent_revisioned_metadata() {
        let (store, a, b) = setup(MessagingLimits::default()).await;
        assert_eq!(
            store.execution_location(&a).await.unwrap(),
            (ExecutionLocation::default(), 0)
        );
        let location = ExecutionLocation {
            connector: Some("forkd".into()),
            ..ExecutionLocation::default()
        };
        assert_eq!(
            store
                .set_execution_location(&a, &location, 0)
                .await
                .unwrap(),
            1
        );
        assert!(store
            .set_execution_location(&a, &location, 0)
            .await
            .is_err());
        assert_eq!(
            store
                .set_execution_location(&a, &location, 1)
                .await
                .unwrap(),
            2
        );
        let policy = WorkReportPolicy {
            revision: 1,
            intermediate: Some(fixed(&b)),
            final_result: Some(DestinationSpec::ChannelActive { route: display() }),
        };
        store.set_report_policy(&a, &policy, &Guard).await.unwrap();
        assert_eq!(store.report_policy(&a).await.unwrap(), Some(policy.clone()));
        assert!(store.set_report_policy(&a, &policy, &Guard).await.is_err());
        assert_eq!(messages(&store).await, 0);
        store.clear_report_policy(&a).await.unwrap();
        assert!(store.report_policy(&a).await.unwrap().is_none());
        let invalid = MessagingLimits {
            max_attempts: 0,
            ..MessagingLimits::default()
        };
        assert!(invalid.validate().is_err());
        let mut forged = to_value(draft(fixed(&b), MessageIntent::Notice)).unwrap();
        forged["actor"] = "owner".into();
        forged["source"] = "forged".into();
        assert!(from_value::<MessageDraft>(forged).is_err());
    }
}
