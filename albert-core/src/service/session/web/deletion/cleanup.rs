use std::path::PathBuf;

use octo_history::SessionId;
use sqlx::{query, query_as};

use crate::{
    db::error::DatabaseError, history::sessions::Backend, service::session::SessionService,
    sessions::error::SessionError, sessions::error::SessionResult,
};

pub(crate) struct ErasureFiles {
    pub workspace: PathBuf,
    pub assets: Vec<(String, PathBuf)>,
}
impl SessionService {
    pub(crate) async fn pending_erasures(&self) -> SessionResult<Vec<SessionId>> {
        let rows: Vec<(String,)> =
            query_as("SELECT session_id FROM albert_session_erasures ORDER BY session_id LIMIT 16")
                .fetch_all(self.database.pool())
                .await
                .map_err(DatabaseError::from)?;
        rows.into_iter()
            .map(|(id,)| SessionId::parse(&id).map_err(SessionError::from))
            .collect()
    }
    pub(crate) async fn erasure_files(
        &self,
        id: &SessionId,
    ) -> SessionResult<Option<ErasureFiles>> {
        let _lifecycle = self.router.lifecycle_gate().write_owned().await;
        let mut backend = self.router.handle(id).await.write_owned().await;
        let mut tx = self.history.begin_journal().await?;
        let row: Option<(String,)> = query_as("SELECT e.workspace_root FROM albert_session_erasures e JOIN albert_session_markers m ON m.session_id=e.session_id WHERE e.session_id=? AND m.state='closed' AND m.owner_user_id IS NOT NULL")
            .bind(id.as_str()).fetch_optional(&mut *tx).await.map_err(DatabaseError::from)?;
        let Some((workspace,)) = row else {
            return Ok(None);
        };
        let assets: Vec<(String, String)> = query_as("SELECT asset_id,workspace_root FROM albert_session_erasure_files WHERE session_id=? ORDER BY asset_id,workspace_root")
            .bind(id.as_str()).fetch_all(&mut *tx).await.map_err(DatabaseError::from)?;
        self.leases.close(id)?;
        *backend = Backend::Closed;
        tx.commit().await.map_err(DatabaseError::from)?;
        Ok(Some(ErasureFiles {
            workspace: PathBuf::from(workspace),
            assets: assets
                .into_iter()
                .map(|(id, root)| (id, PathBuf::from(root)))
                .collect(),
        }))
    }
    pub(crate) async fn acknowledge_erasure(&self, id: &SessionId) -> SessionResult<()> {
        query("DELETE FROM albert_session_erasures WHERE session_id=?")
            .bind(id.as_str())
            .execute(self.database.pool())
            .await
            .map_err(DatabaseError::from)?;
        Ok(())
    }
}
