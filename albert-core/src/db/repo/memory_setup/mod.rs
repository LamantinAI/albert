//! The selected memory backend remains in TOML; SQL stores only durable setup
//! consent. Initialization failures never discard that choice or infer a reset.
use sqlx::{query, query_as, query_scalar, SqlitePool};

use crate::db::error::{DatabaseError, DatabaseResult};

#[derive(Clone)]
pub struct MemorySetup {
    pub state: String,
    pub revision: i64,
}
impl MemorySetup {
    pub fn selected(&self) -> bool {
        matches!(self.state.as_str(), "adopted" | "selected")
    }
}
pub async fn read(pool: &SqlitePool) -> DatabaseResult<MemorySetup> {
    let row: (String, i64) =
        query_as("SELECT state,revision FROM albert_memory_setup WHERE singleton=1")
            .fetch_one(pool)
            .await?;
    Ok(MemorySetup {
        state: row.0,
        revision: row.1,
    })
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StartupPolicy {
    BrowserOnboarding,
    LegacyProvisioned,
    ExplicitMemory,
}

/// An explicit startup file may complete a pending choice. A newly saved model
/// alone must never reclassify a pending browser flow.
pub async fn initialize(pool: &SqlitePool, policy: StartupPolicy) -> DatabaseResult<MemorySetup> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let retained: bool =
        query_scalar("SELECT EXISTS(SELECT 1 FROM turns) OR EXISTS(SELECT 1 FROM compacts)")
            .fetch_one(&mut *tx)
            .await?;
    let state = if policy != StartupPolicy::BrowserOnboarding || retained {
        "adopted"
    } else {
        "pending"
    };
    query(
        "UPDATE albert_memory_setup SET state=?,revision=revision+1 WHERE singleton=1
         AND (state='uninitialized' OR (state='pending' AND ?))",
    )
    .bind(state)
    .bind(policy == StartupPolicy::ExplicitMemory)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    read(pool).await
}
pub async fn accept(pool: &SqlitePool, expected_revision: i64) -> DatabaseResult<MemorySetup> {
    let changed = query("UPDATE albert_memory_setup SET state='selected',revision=revision+1 WHERE singleton=1 AND revision=?")
        .bind(expected_revision).execute(pool).await?;
    if changed.rows_affected() != 1 {
        return Err(DatabaseError::RevisionConflict);
    }
    read(pool).await
}

#[cfg(test)]
mod tests {
    use octo_history::{HistoryStore, SqliteHistory, Turn};
    use sqlx::{query, raw_sql};
    use std::sync::Arc;
    use tempfile::tempdir;

    use super::{accept, initialize, read, StartupPolicy};
    use crate::db::Database;

    #[tokio::test]
    async fn fresh_selection_stays_pending_across_model_changes_and_reopen() {
        let dir = tempdir().unwrap();
        let history = Arc::new(
            SqliteHistory::open_retained(dir.path().join("history.db"))
                .await
                .unwrap(),
        );
        let db = Database::from_pool(history.pool().clone()).await.unwrap();
        assert_eq!(read(db.pool()).await.unwrap().state, "uninitialized");
        let pending = initialize(db.pool(), StartupPolicy::BrowserOnboarding)
            .await
            .unwrap();
        assert!(!pending.selected());
        assert_eq!(
            initialize(db.pool(), StartupPolicy::LegacyProvisioned)
                .await
                .unwrap()
                .state,
            "pending"
        );
        Database::from_pool(history.pool().clone()).await.unwrap();
        assert_eq!(read(db.pool()).await.unwrap().state, "pending");
        let selected = accept(db.pool(), pending.revision).await.unwrap();
        assert!(selected.selected());
        assert!(accept(db.pool(), pending.revision).await.is_err());
        assert!(initialize(db.pool(), StartupPolicy::BrowserOnboarding)
            .await
            .unwrap()
            .selected());
    }
    #[tokio::test]
    async fn provisioned_and_retained_installations_adopt_existing_memory() {
        for provisioned in [false, true] {
            let dir = tempdir().unwrap();
            let history = SqliteHistory::open_retained(dir.path().join("history.db"))
                .await
                .unwrap();
            let db = Database::from_pool(history.pool().clone()).await.unwrap();
            if !provisioned {
                history
                    .append("legacy-channel", &[Turn::user("existing context")])
                    .await
                    .unwrap();
            }
            assert!(initialize(
                db.pool(),
                if provisioned {
                    StartupPolicy::LegacyProvisioned
                } else {
                    StartupPolicy::BrowserOnboarding
                }
            )
            .await
            .unwrap()
            .selected());
        }
    }
    #[tokio::test]
    async fn upgrading_an_existing_ledger_adopts_memory_even_without_current_credentials() {
        let dir = tempdir().unwrap();
        let history = SqliteHistory::open_retained(dir.path().join("history.db"))
            .await
            .unwrap();
        let db = Database::from_pool(history.pool().clone()).await.unwrap();
        query("DROP TABLE albert_memory_setup")
            .execute(db.pool())
            .await
            .unwrap();
        query("DROP TABLE albert_model_usage")
            .execute(db.pool())
            .await
            .unwrap();
        query("DROP TABLE albert_usage_tracking")
            .execute(db.pool())
            .await
            .unwrap();
        query("DROP TABLE albert_media_assets")
            .execute(db.pool())
            .await
            .unwrap();
        raw_sql(include_str!(
            "../../../../migrations/0010_session_media.sql"
        ))
        .execute(db.pool())
        .await
        .unwrap();
        query("DELETE FROM albert_schema_migrations WHERE version>=13")
            .execute(db.pool())
            .await
            .unwrap();
        Database::from_pool(history.pool().clone()).await.unwrap();
        assert_eq!(read(db.pool()).await.unwrap().state, "adopted");
        assert!(initialize(db.pool(), StartupPolicy::BrowserOnboarding)
            .await
            .unwrap()
            .selected());
    }
}
