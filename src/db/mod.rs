//! Internal application metadata. Session catalogs and journals belong to
//! octo-history; user-created SQL stores belong to the isolated SQL connector.

pub(crate) mod entity;
pub(crate) mod error;
pub(crate) mod repo;

use sha2::{Digest, Sha256};
use sqlx::{query, query_scalar, raw_sql, SqlitePool};

use crate::db::error::{DatabaseError, DatabaseResult};

const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../../migrations/0001_session_control.sql")),
    (2, include_str!("../../migrations/0002_native_sessions.sql")),
    (
        3,
        include_str!("../../migrations/0003_session_messaging.sql"),
    ),
    (
        4,
        include_str!("../../migrations/0004_message_failure_notices.sql"),
    ),
    (
        5,
        include_str!("../../migrations/0005_message_delivery_evidence.sql"),
    ),
];
const SCHEMA_VERSION: i64 = 5;

#[derive(Clone)]
pub(crate) struct Database {
    pool: SqlitePool,
}

impl Database {
    pub const fn schema_version() -> i64 {
        SCHEMA_VERSION
    }

    pub async fn from_pool(pool: SqlitePool) -> DatabaseResult<Self> {
        let db = Self { pool };
        db.migrate().await?;
        Ok(db)
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    async fn migrate(&self) -> DatabaseResult<()> {
        // Use our own registry, never octo-history's `_sqlx_migrations`. All
        // schema changes and their checksum record commit or roll back together.
        let mut tx = self.pool.begin().await?;
        query(
            "CREATE TABLE IF NOT EXISTS albert_schema_migrations \
             (version INTEGER PRIMARY KEY, checksum TEXT NOT NULL)",
        )
        .execute(&mut *tx)
        .await?;
        let latest: Option<i64> = query_scalar("SELECT MAX(version) FROM albert_schema_migrations")
            .fetch_one(&mut *tx)
            .await?;
        if latest.is_some_and(|version| version > SCHEMA_VERSION) {
            return Err(DatabaseError::SchemaVersion(latest.unwrap()));
        }
        for &(version, sql) in MIGRATIONS {
            let checksum = format!("{:x}", Sha256::digest(sql.as_bytes()));
            let previous: Option<String> =
                query_scalar("SELECT checksum FROM albert_schema_migrations WHERE version = ?")
                    .bind(version)
                    .fetch_optional(&mut *tx)
                    .await?;
            match previous {
                Some(previous) if previous != checksum => {
                    return Err(DatabaseError::MigrationChecksum(version));
                }
                Some(_) => {}
                None => {
                    raw_sql(sql).execute(&mut *tx).await?;
                    query("INSERT INTO albert_schema_migrations (version, checksum) VALUES (?, ?)")
                        .bind(version)
                        .bind(checksum)
                        .execute(&mut *tx)
                        .await?;
                }
            }
        }
        tx.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use sqlx::{query, query_scalar, sqlite::SqlitePoolOptions};

    use super::Database;
    use crate::db::{
        entity::{MarkerState, PrivateRouteKey},
        error::DatabaseError,
        repo::{identity::bind, marker::set as set_marker, route::set as set_route},
    };

    async fn database() -> Database {
        Database::from_pool(
            SqlitePoolOptions::new()
                .max_connections(1)
                .connect("sqlite::memory:")
                .await
                .unwrap(),
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn migrations_are_repeatable_and_do_not_claim_history_registry() {
        let db = database().await;
        Database::from_pool(db.pool().clone()).await.unwrap();
        let count: i64 = query_scalar("SELECT COUNT(*) FROM albert_schema_migrations")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(count, 5);
        let history_registry: i64 =
            query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE name = '_sqlx_migrations'")
                .fetch_one(db.pool())
                .await
                .unwrap();
        assert_eq!(history_registry, 0);
        query("UPDATE albert_schema_migrations SET checksum = 'changed'")
            .execute(db.pool())
            .await
            .unwrap();
        assert!(matches!(
            Database::from_pool(db.pool().clone()).await,
            Err(DatabaseError::MigrationChecksum(1))
        ));
    }

    #[tokio::test]
    async fn conflicting_identity_cannot_reassign_an_existing_user() {
        let db = database().await;
        let mut tx = db.pool().begin().await.unwrap();
        bind(&mut tx, "telegram", "7", "user-a").await.unwrap();
        tx.commit().await.unwrap();
        let mut tx = db.pool().begin().await.unwrap();
        assert!(matches!(
            bind(&mut tx, "telegram", "7", "user-b").await,
            Err(DatabaseError::IdentityConflict)
        ));
        tx.rollback().await.unwrap();
        let users: i64 = query_scalar("SELECT COUNT(*) FROM albert_users")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(users, 1);
    }

    #[tokio::test]
    async fn selector_and_content_free_marker_share_rollback_and_cas() {
        let db = database().await;
        let key = PrivateRouteKey {
            connector_id: "telegram".into(),
            channel_id: "7".into(),
            user_id: "owner".into(),
        };
        let mut tx = db.pool().begin().await.unwrap();
        bind(&mut tx, "telegram", "7", "owner").await.unwrap();
        set_route(&mut tx, &key, "original", None, None)
            .await
            .unwrap();
        tx.commit().await.unwrap();

        let mut tx = db.pool().begin().await.unwrap();
        set_route(&mut tx, &key, "temporary", Some("original"), Some(1))
            .await
            .unwrap();
        set_marker(
            &mut tx,
            "temporary",
            "epoch-a",
            MarkerState::Temporary,
            None,
        )
        .await
        .unwrap();
        tx.rollback().await.unwrap();
        let route: String = query_scalar("SELECT session_id FROM albert_private_session_routes")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(route, "original");
        let markers: i64 = query_scalar("SELECT COUNT(*) FROM albert_session_markers")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(markers, 0);

        let mut tx = db.pool().begin().await.unwrap();
        assert!(matches!(
            set_route(&mut tx, &key, "changed", None, Some(99)).await,
            Err(DatabaseError::RevisionConflict)
        ));
        tx.rollback().await.unwrap();
    }
}
