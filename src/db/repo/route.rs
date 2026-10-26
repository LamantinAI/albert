use sqlx::{query, Row, Sqlite, SqlitePool, Transaction};

use crate::db::{
    entity::{PrivateRoute, PrivateRouteKey},
    error::{DatabaseError, DatabaseResult},
};

pub(crate) async fn find(
    pool: &SqlitePool,
    key: &PrivateRouteKey,
) -> DatabaseResult<Option<PrivateRoute>> {
    let row = query(
        "SELECT session_id, return_session_id, revision FROM albert_private_session_routes \
         WHERE connector_id = ? AND channel_id = ? AND user_id = ?",
    )
    .bind(&key.connector_id)
    .bind(&key.channel_id)
    .bind(&key.user_id)
    .fetch_optional(pool)
    .await?;
    row.map(|r| {
        Ok(PrivateRoute {
            session_id: r.try_get("session_id")?,
            return_session_id: r.try_get("return_session_id")?,
            revision: r.try_get("revision")?,
        })
    })
    .transpose()
}

pub(crate) async fn find_in(
    tx: &mut Transaction<'_, Sqlite>,
    key: &PrivateRouteKey,
) -> DatabaseResult<Option<PrivateRoute>> {
    let row = query(
        "SELECT session_id, return_session_id, revision FROM albert_private_session_routes \
         WHERE connector_id = ? AND channel_id = ? AND user_id = ?",
    )
    .bind(&key.connector_id)
    .bind(&key.channel_id)
    .bind(&key.user_id)
    .fetch_optional(&mut **tx)
    .await?;
    row.map(|r| {
        Ok(PrivateRoute {
            session_id: r.try_get("session_id")?,
            return_session_id: r.try_get("return_session_id")?,
            revision: r.try_get("revision")?,
        })
    })
    .transpose()
}

/// `None` creates a new selector; `Some` performs compare-and-swap. Callers
/// validate session access against octo-history and own the surrounding commit.
pub(crate) async fn set(
    tx: &mut Transaction<'_, Sqlite>,
    key: &PrivateRouteKey,
    session_id: &str,
    return_session_id: Option<&str>,
    expected_revision: Option<i64>,
) -> DatabaseResult<i64> {
    let changed = if let Some(revision) = expected_revision {
        query(
            "UPDATE albert_private_session_routes \
             SET session_id = ?, return_session_id = ?, revision = revision + 1 \
             WHERE connector_id = ? AND channel_id = ? AND user_id = ? AND revision = ?",
        )
        .bind(session_id)
        .bind(return_session_id)
        .bind(&key.connector_id)
        .bind(&key.channel_id)
        .bind(&key.user_id)
        .bind(revision)
        .execute(&mut **tx)
        .await?
    } else {
        query(
            "INSERT INTO albert_private_session_routes \
             (connector_id, channel_id, user_id, session_id, return_session_id, revision) \
             VALUES (?, ?, ?, ?, ?, 1) \
             ON CONFLICT (connector_id, channel_id, user_id) DO NOTHING",
        )
        .bind(&key.connector_id)
        .bind(&key.channel_id)
        .bind(&key.user_id)
        .bind(session_id)
        .bind(return_session_id)
        .execute(&mut **tx)
        .await?
    };
    if changed.rows_affected() != 1 {
        return Err(DatabaseError::RevisionConflict);
    }
    Ok(expected_revision.map_or(1, |r| r + 1))
}
