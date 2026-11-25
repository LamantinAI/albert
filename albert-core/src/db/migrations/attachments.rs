//! File preparation for existing media rows, before the final SQL step.
use sqlx::{query, query_as, Sqlite, Transaction};
use tokio::task::spawn_blocking;

use crate::{
    db::error::{DatabaseError, DatabaseResult},
    media::AttachmentFiles,
};

pub(super) async fn extract_attachments(
    tx: &mut Transaction<'_, Sqlite>,
    files: Option<&AttachmentFiles>,
) -> DatabaseResult<()> {
    let mut after: Option<String> = None;
    loop {
        let row: Option<(String, String, i64, Vec<u8>)> = query_as(
            "SELECT id,session_id,size,bytes FROM albert_media_assets \
             WHERE (? IS NULL OR id>?) ORDER BY id LIMIT 1",
        )
        .bind(&after)
        .bind(&after)
        .fetch_optional(&mut **tx)
        .await?;
        let Some((id, session, size, bytes)) = row else {
            return Ok(());
        };
        let store = files.ok_or(DatabaseError::AttachmentMigration)?.clone();
        if i64::try_from(bytes.len()).ok() != Some(size) {
            return Err(DatabaseError::AttachmentMigration);
        }
        let file_id = id.clone();
        let sha256 = spawn_blocking(move || store.store(&session, &file_id, &bytes))
            .await
            .map_err(|_| DatabaseError::AttachmentMigration)??;
        query("UPDATE albert_media_assets SET sha256=? WHERE id=?")
            .bind(sha256)
            .bind(&id)
            .execute(&mut **tx)
            .await?;
        after = Some(id);
    }
}
