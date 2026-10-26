use sqlx::{query, Row, Sqlite, SqlitePool, Transaction};

use crate::db::{
    entity::{MarkerState, SessionMarker},
    error::{DatabaseError, DatabaseResult},
};

pub(crate) async fn find(
    pool: &SqlitePool,
    session_id: &str,
) -> DatabaseResult<Option<SessionMarker>> {
    let row = query(
        "SELECT session_id, runtime_epoch, state, revision, owner_user_id \
         FROM albert_session_markers WHERE session_id = ?",
    )
    .bind(session_id)
    .fetch_optional(pool)
    .await?;
    row.map(|r| {
        Ok(SessionMarker {
            owner_user_id: r.try_get("owner_user_id")?,
            session_id: r.try_get("session_id")?,
            runtime_epoch: r.try_get("runtime_epoch")?,
            state: MarkerState::parse(r.try_get("state")?)?,
            revision: r.try_get("revision")?,
        })
    })
    .transpose()
}

pub(crate) async fn find_in(
    tx: &mut Transaction<'_, Sqlite>,
    session_id: &str,
) -> DatabaseResult<Option<SessionMarker>> {
    let row = query("SELECT session_id, runtime_epoch, state, revision, owner_user_id FROM albert_session_markers WHERE session_id = ?")
        .bind(session_id).fetch_optional(&mut **tx).await?;
    row.map(|r| {
        Ok(SessionMarker {
            owner_user_id: r.try_get("owner_user_id")?,
            session_id: r.try_get("session_id")?,
            runtime_epoch: r.try_get("runtime_epoch")?,
            state: MarkerState::parse(r.try_get("state")?)?,
            revision: r.try_get("revision")?,
        })
    })
    .transpose()
}

pub(crate) async fn all(pool: &SqlitePool) -> DatabaseResult<Vec<SessionMarker>> {
    let rows = query("SELECT session_id, runtime_epoch, state, revision, owner_user_id FROM albert_session_markers")
        .fetch_all(pool).await?;
    rows.into_iter()
        .map(|r| {
            Ok(SessionMarker {
                owner_user_id: r.try_get("owner_user_id")?,
                session_id: r.try_get("session_id")?,
                runtime_epoch: r.try_get("runtime_epoch")?,
                state: MarkerState::parse(r.try_get("state")?)?,
                revision: r.try_get("revision")?,
            })
        })
        .collect()
}

pub(crate) async fn expire_previous_runtime(
    tx: &mut Transaction<'_, Sqlite>,
    runtime_epoch: &str,
) -> DatabaseResult<()> {
    query("UPDATE albert_session_markers SET state = 'expired', revision = revision + 1 WHERE state = 'temporary' AND runtime_epoch != ?")
        .bind(runtime_epoch).execute(&mut **tx).await?;
    Ok(())
}

pub(crate) async fn set(
    tx: &mut Transaction<'_, Sqlite>,
    session_id: &str,
    runtime_epoch: &str,
    state: MarkerState,
    expected_revision: Option<i64>,
) -> DatabaseResult<i64> {
    let changed = if let Some(revision) = expected_revision {
        query(
            "UPDATE albert_session_markers SET runtime_epoch = ?, state = ?, \
             revision = revision + 1 WHERE session_id = ? AND revision = ?",
        )
        .bind(runtime_epoch)
        .bind(state.as_str())
        .bind(session_id)
        .bind(revision)
        .execute(&mut **tx)
        .await?
    } else {
        query(
            "INSERT INTO albert_session_markers (session_id, runtime_epoch, state, revision) \
             VALUES (?, ?, ?, 1) ON CONFLICT (session_id) DO NOTHING",
        )
        .bind(session_id)
        .bind(runtime_epoch)
        .bind(state.as_str())
        .execute(&mut **tx)
        .await?
    };
    if changed.rows_affected() != 1 {
        return Err(DatabaseError::RevisionConflict);
    }
    Ok(expected_revision.map_or(1, |r| r + 1))
}

pub(crate) async fn set_owned(
    tx: &mut Transaction<'_, Sqlite>,
    session_id: &str,
    runtime_epoch: &str,
    state: MarkerState,
    expected_revision: Option<i64>,
    owner_user_id: &str,
) -> DatabaseResult<i64> {
    let revision = set(tx, session_id, runtime_epoch, state, expected_revision).await?;
    let changed = query("UPDATE albert_session_markers SET owner_user_id = ? WHERE session_id = ? AND (owner_user_id IS NULL OR owner_user_id = ?)")
        .bind(owner_user_id).bind(session_id).bind(owner_user_id).execute(&mut **tx).await?;
    if changed.rows_affected() != 1 {
        return Err(DatabaseError::IdentityConflict);
    }
    Ok(revision)
}
