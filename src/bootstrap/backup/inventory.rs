use std::path::Path;

use sha2::{Digest, Sha256};
use sqlx::{query_as, query_scalar, SqlitePool};

use crate::{db::error::DatabaseError, error::Result};

/// Content-free inventory identifies the preparatory snapshot for remaining
/// legacy journals. Unchanged technical orphans do not produce endless backups.
pub(super) async fn legacy_fingerprint(pool: &SqlitePool, path: &Path) -> Result<Option<String>> {
    let mut digest = Sha256::new();
    digest.update(path.as_os_str().as_encoded_bytes());
    let bound: bool = query_scalar("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='history_session_journals')")
        .fetch_one(pool).await.map_err(DatabaseError::from)?;
    let mut any = false;
    for table in ["turns", "compacts", "journal_appends"] {
        let exists: bool = query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?)",
        )
        .bind(table)
        .fetch_one(pool)
        .await
        .map_err(DatabaseError::from)?;
        if !exists {
            continue;
        }
        // Fixed schema identifiers, never configuration or native message data.
        let predicate = if bound {
            "WHERE channel NOT IN (SELECT history_key FROM history_session_journals)"
        } else {
            ""
        };
        let statement = format!("SELECT channel,COUNT(*),MAX(id) FROM {table} {predicate} GROUP BY channel ORDER BY channel");
        let rows: Vec<(String, i64, i64)> = query_as(&statement)
            .fetch_all(pool)
            .await
            .map_err(DatabaseError::from)?;
        digest.update(table.as_bytes());
        for (key, count, last_id) in rows {
            any = true;
            digest.update((key.len() as u64).to_be_bytes());
            digest.update(key.as_bytes());
            digest.update(count.to_be_bytes());
            digest.update(last_id.to_be_bytes());
        }
    }
    Ok(any.then(|| format!("{:x}", digest.finalize())))
}
