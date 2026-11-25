//! Application metadata cleanup; transcript ownership stays in octo-history.
use sqlx::{query, Sqlite, Transaction};

use crate::db::error::DatabaseResult;

pub(crate) async fn erase_in(
    tx: &mut Transaction<'_, Sqlite>,
    session: &str,
    workspace: &str,
) -> DatabaseResult<()> {
    query("INSERT INTO albert_session_erasures(session_id,workspace_root) VALUES (?,?) ON CONFLICT(session_id) DO NOTHING")
        .bind(session).bind(workspace).execute(&mut **tx).await?;
    query("INSERT OR IGNORE INTO albert_session_erasure_files(session_id,asset_id,workspace_root) SELECT session_id,asset_id,workspace_root FROM albert_session_workspace_copies WHERE session_id=?")
        .bind(session).execute(&mut **tx).await?;
    query("INSERT OR IGNORE INTO albert_session_erasure_files(session_id,asset_id,workspace_root) SELECT a.session_id,a.id,? FROM albert_media_assets a WHERE a.session_id=? AND NOT EXISTS(SELECT 1 FROM albert_session_workspace_copies c WHERE c.asset_id=a.id)")
        .bind(workspace).bind(session).execute(&mut **tx).await?;
    query("DELETE FROM albert_session_workspace_copies WHERE session_id=?")
        .bind(session)
        .execute(&mut **tx)
        .await?;
    query("DELETE FROM albert_media_assets WHERE session_id=?")
        .bind(session)
        .execute(&mut **tx)
        .await?;
    for statement in [
        "DELETE FROM albert_message_inbox WHERE message_id IN (SELECT message_id FROM albert_messages WHERE source_session=?)",
        "DELETE FROM albert_message_outbox WHERE message_id IN (SELECT message_id FROM albert_messages WHERE source_session=?)",
        "DELETE FROM albert_message_delivery_evidence WHERE message_id IN (SELECT message_id FROM albert_messages WHERE source_session=?)",
        "DELETE FROM albert_message_report_policies WHERE message_id IN (SELECT message_id FROM albert_messages WHERE source_session=?)",
        "DELETE FROM albert_messages WHERE source_session=?",
        "DELETE FROM albert_message_inbox WHERE recipient_session=?",
        "DELETE FROM albert_session_execution WHERE session_id=?",
        "DELETE FROM albert_fork_name_sequences WHERE session_id=?",
        "DELETE FROM albert_work_reports WHERE session_id=?",
        "UPDATE albert_web_starts SET phase='expired' WHERE session_id=?",
    ] {
        query(statement).bind(session).execute(&mut **tx).await?;
    }
    Ok(())
}
