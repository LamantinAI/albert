use sqlx::{query, sqlite::SqliteRow, Row, Sqlite, Transaction};

use crate::db::{
    entity::{Excursion, ExcursionKey, NativePosition},
    error::{DatabaseError, DatabaseResult},
};

pub(crate) async fn list_in(
    tx: &mut Transaction<'_, Sqlite>,
    key: &ExcursionKey,
) -> DatabaseResult<Vec<Excursion>> {
    let rows = query("SELECT * FROM albert_temporary_excursions WHERE connector_id=? AND bot_id=? AND chat_id=? AND actor_id=? ORDER BY start_date,start_update_id")
        .bind(&key.connector_id).bind(&key.bot_id).bind(&key.chat_id).bind(&key.actor_id)
        .fetch_all(&mut **tx).await?;
    rows.into_iter().map(decode).collect()
}

pub(crate) async fn insert_in(
    tx: &mut Transaction<'_, Sqlite>,
    excursion: &Excursion,
) -> DatabaseResult<()> {
    query("INSERT INTO albert_temporary_excursions (connector_id,bot_id,chat_id,actor_id,user_id,session_id,return_session_id,scope_epoch,start_date,start_update_id,start_message_id,disposition,revision) VALUES (?,?,?,?,?,?,?,?,?,?,?,'active',1)")
        .bind(&excursion.key.connector_id).bind(&excursion.key.bot_id)
        .bind(&excursion.key.chat_id).bind(&excursion.key.actor_id)
        .bind(&excursion.user_id).bind(&excursion.session_id).bind(&excursion.return_session_id)
        .bind(&excursion.scope_epoch).bind(excursion.start.date).bind(excursion.start.update_id)
        .bind(excursion.start_message_id).execute(&mut **tx).await?;
    Ok(())
}

pub(crate) async fn promote_in(
    tx: &mut Transaction<'_, Sqlite>,
    session_id: &str,
) -> DatabaseResult<()> {
    query("UPDATE albert_temporary_excursions SET disposition='promoted',revision=revision+1 WHERE session_id=? AND disposition='active'")
        .bind(session_id).execute(&mut **tx).await?;
    Ok(())
}

pub(crate) async fn discard_in(
    tx: &mut Transaction<'_, Sqlite>,
    session_id: &str,
) -> DatabaseResult<()> {
    query("UPDATE albert_temporary_excursions SET disposition='discarded',revision=revision+1 WHERE session_id=? AND disposition='active'")
        .bind(session_id).execute(&mut **tx).await?;
    Ok(())
}

pub(crate) async fn discard_expired_in(tx: &mut Transaction<'_, Sqlite>) -> DatabaseResult<()> {
    query("UPDATE albert_temporary_excursions SET disposition='discarded',revision=revision+1 WHERE disposition='active' AND session_id IN (SELECT session_id FROM albert_session_markers WHERE state IN ('expired','closed'))")
        .execute(&mut **tx).await?;
    Ok(())
}

pub(crate) async fn close_in(
    tx: &mut Transaction<'_, Sqlite>,
    excursion: &Excursion,
    end: NativePosition,
    message_id: i64,
) -> DatabaseResult<()> {
    if end < excursion.start {
        return Err(DatabaseError::RevisionConflict);
    }
    let changed = query("UPDATE albert_temporary_excursions SET end_date=?,end_update_id=?,end_message_id=?,revision=revision+1 WHERE session_id=? AND connector_id=? AND bot_id=? AND chat_id=? AND actor_id=? AND revision=? AND end_date IS NULL")
        .bind(end.date).bind(end.update_id).bind(message_id).bind(&excursion.session_id)
        .bind(&excursion.key.connector_id).bind(&excursion.key.bot_id)
        .bind(&excursion.key.chat_id).bind(&excursion.key.actor_id).bind(excursion.revision)
        .execute(&mut **tx).await?;
    if changed.rows_affected() != 1 {
        return Err(DatabaseError::RevisionConflict);
    }
    Ok(())
}

fn decode(row: SqliteRow) -> DatabaseResult<Excursion> {
    let end_date: Option<i64> = row.try_get("end_date")?;
    let end_update: Option<i64> = row.try_get("end_update_id")?;
    let end = match (end_date, end_update) {
        (Some(date), Some(update_id)) => Some(NativePosition { date, update_id }),
        (None, None) => None,
        _ => return Err(DatabaseError::MarkerState),
    };
    Ok(Excursion {
        key: ExcursionKey {
            connector_id: row.try_get("connector_id")?,
            bot_id: row.try_get("bot_id")?,
            chat_id: row.try_get("chat_id")?,
            actor_id: row.try_get("actor_id")?,
        },
        user_id: row.try_get("user_id")?,
        session_id: row.try_get("session_id")?,
        return_session_id: row.try_get("return_session_id")?,
        scope_epoch: row.try_get("scope_epoch")?,
        start: NativePosition {
            date: row.try_get("start_date")?,
            update_id: row.try_get("start_update_id")?,
        },
        start_message_id: row.try_get("start_message_id")?,
        end,
        end_message_id: row.try_get("end_message_id")?,
        disposition: row.try_get("disposition")?,
        revision: row.try_get("revision")?,
    })
}
