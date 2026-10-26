#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    fs::{canonicalize, create_dir_all, remove_file, rename, set_permissions, Permissions},
    path::{Path, PathBuf},
};

use chrono::Utc;
use octo_history::SqliteHistory;
use rand::random;
use sqlx::{
    query, query_scalar,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};

use self::inventory::legacy_fingerprint;
use crate::{
    db::{error::DatabaseError, Database},
    error::{Error, Result},
};

/// Read the original file/WAL through SQLite, before either migration owner opens it.
/// VACUUM INTO yields a consistent committed snapshot; copying the main file does not.
pub(super) async fn before_migration(path: &Path, backup_dir: &Path) -> Result<Option<PathBuf>> {
    if !path.exists() || path.metadata()?.len() == 0 {
        return Ok(None);
    }
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(SqliteConnectOptions::new().filename(path).read_only(true))
        .await
        .map_err(DatabaseError::from)?;
    let result = async {
        let mut needs_backup = false;
        for (table, target) in [("_sqlx_migrations", SqliteHistory::schema_version()), ("albert_schema_migrations", Database::schema_version())] {
            let exists: bool = query_scalar("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?)")
                .bind(table).fetch_one(&pool).await.map_err(DatabaseError::from)?;
            let version: Option<i64> = if exists {
                // The identifiers are fixed host constants, never configuration/input.
                query_scalar(if table == "_sqlx_migrations" { "SELECT MAX(version) FROM _sqlx_migrations WHERE success=1" } else { "SELECT MAX(version) FROM albert_schema_migrations" })
                    .fetch_one(&pool).await.map_err(DatabaseError::from)?
            } else { None };
            if version.is_some_and(|version| version > target) {
                return Err(Error::Config(format!("Internal database registry {table} is newer than this binary; startup did not migrate it.")));
            }
            needs_backup |= version.unwrap_or(0) < target;
        }
        let legacy = legacy_fingerprint(&pool, &canonicalize(path)?).await?;
        let legacy_snapshot = legacy.map(|fingerprint| backup_dir.join(format!("history-before-legacy-{fingerprint}.sqlite3")));
        let pending_legacy = legacy_snapshot.as_ref().filter(|snapshot| !snapshot.exists());
        if !needs_backup && pending_legacy.is_none() { return Ok(None); }
        create_dir_all(backup_dir)?;
        #[cfg(unix)]
        set_permissions(backup_dir, Permissions::from_mode(0o700))?;
        let destination = pending_legacy.cloned().unwrap_or_else(|| backup_dir.join(format!("history-before-migration-{}-{:016x}.sqlite3", Utc::now().format("%Y%m%dT%H%M%SZ"), random::<u64>())));
        let temporary = backup_dir.join(format!(".snapshot-{:032x}.tmp", random::<u128>()));
        let filename = temporary.to_str().ok_or_else(|| Error::Config("Backup path must be valid UTF-8.".into()))?;
        if let Err(error) = query("VACUUM INTO ?").bind(filename).execute(&pool).await {
            let _ = remove_file(&temporary);
            return Err(DatabaseError::from(error).into());
        }
        #[cfg(unix)]
        set_permissions(&temporary, Permissions::from_mode(0o600))?;
        rename(&temporary, &destination)?;
        Ok(Some(destination))
    }.await;
    pool.close().await;
    result
}

#[cfg(test)]
mod tests {
    use std::fs::{read_dir, write};

    use octo_history::{HistoryStore, SqliteHistory, Turn};
    use sqlx::{
        query, query_scalar,
        sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
    };
    use tempfile::tempdir;

    use super::before_migration;
    use crate::db::Database;

    #[tokio::test]
    async fn backup_includes_wal_and_precedes_both_migration_chains() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("history.db");
        let backups = dir.path().join("backups");
        let writer = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(&path)
                    .create_if_missing(true)
                    .journal_mode(SqliteJournalMode::Wal),
            )
            .await
            .unwrap();
        query("CREATE TABLE original (value TEXT)")
            .execute(&writer)
            .await
            .unwrap();
        query("INSERT INTO original VALUES ('committed in WAL')")
            .execute(&writer)
            .await
            .unwrap();
        let snapshot = before_migration(&path, &backups).await.unwrap().unwrap();
        let read = SqlitePoolOptions::new()
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(snapshot)
                    .read_only(true),
            )
            .await
            .unwrap();
        assert_eq!(
            query_scalar::<_, String>("SELECT value FROM original")
                .fetch_one(&read)
                .await
                .unwrap(),
            "committed in WAL"
        );
        assert_eq!(query_scalar::<_, i64>("SELECT COUNT(*) FROM sqlite_master WHERE name IN ('_sqlx_migrations','albert_schema_migrations')").fetch_one(&read).await.unwrap(), 0);
        read.close().await;
        writer.close().await;
        let history = SqliteHistory::open_retained(&path).await.unwrap();
        Database::from_pool(history.pool().clone()).await.unwrap();
        assert!(before_migration(&path, &backups).await.unwrap().is_none());
        assert_eq!(read_dir(&backups).unwrap().count(), 1);
    }

    #[tokio::test]
    async fn missing_or_current_file_is_not_backed_up_but_metadata_upgrade_is() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("history.db");
        let backups = dir.path().join("backups");
        assert!(before_migration(&path, &backups).await.unwrap().is_none());
        let history = SqliteHistory::open_retained(&path).await.unwrap();
        assert!(before_migration(&path, &backups).await.unwrap().is_some());
        Database::from_pool(history.pool().clone()).await.unwrap();
        assert!(before_migration(&path, &backups).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn current_schema_legacy_data_gets_one_snapshot_and_failure_stops_preflight() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("history.db");
        let backups = dir.path().join("backups");
        let history = SqliteHistory::open_retained(&path).await.unwrap();
        Database::from_pool(history.pool().clone()).await.unwrap();
        history
            .append("77", &[Turn::user("old non-owner chat")])
            .await
            .unwrap();
        history
            .append("unknown-leftover", &[Turn::user("later lazy migration")])
            .await
            .unwrap();
        let snapshot = before_migration(&path, &backups).await.unwrap().unwrap();
        assert!(snapshot.exists());
        assert!(before_migration(&path, &backups).await.unwrap().is_none());
        assert_eq!(read_dir(&backups).unwrap().count(), 1);
        history
            .append("88", &[Turn::user("another legacy source")])
            .await
            .unwrap();
        let invalid_backup_dir = dir.path().join("not-a-directory");
        write(&invalid_backup_dir, "existing file").unwrap();
        assert!(before_migration(&path, &invalid_backup_dir).await.is_err());
        assert_eq!(history.load("88").await.len(), 1);
    }
}
mod inventory;
