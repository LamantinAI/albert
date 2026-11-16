use std::{env::current_dir, path::Path};

use octo_history::SessionId;
use sqlx::{query, query_scalar, Sqlite, Transaction};

use super::MediaStore;
use crate::media::{MediaError, MediaResult};

impl MediaStore {
    /// The host has verified the actor and holds an owned session backend lease
    /// through its file worker. This stores only managed-copy ownership, never
    /// promotes staged bytes or claims that a message has been accepted.
    pub async fn register_workspace_copies(
        &self,
        session: &str,
        assets: &[String],
        root: &Path,
    ) -> MediaResult<()> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        self.register_workspace_copies_in(&mut tx, session, assets, root)
            .await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn register_workspace_copies_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        session: &str,
        assets: &[String],
        root: &Path,
    ) -> MediaResult<()> {
        if assets.is_empty() {
            return Ok(());
        }
        SessionId::parse(session).map_err(|_| MediaError::Scope)?;
        let root = if root.is_absolute() {
            root.to_owned()
        } else {
            current_dir().map_err(|_| MediaError::Workspace)?.join(root)
        };
        let root = root.to_str().ok_or(MediaError::Workspace)?;
        let mut count: i64 = query_scalar("SELECT count(*) FROM albert_session_workspace_copies")
            .fetch_one(&mut **tx)
            .await?;
        for asset in assets {
            SessionId::parse(asset).map_err(|_| MediaError::Scope)?;
            let conflicting: bool = query_scalar("SELECT EXISTS(SELECT 1 FROM albert_session_workspace_copies WHERE asset_id=? AND session_id!=?)")
                .bind(asset).bind(session).fetch_one(&mut **tx).await?;
            if conflicting {
                return Err(MediaError::Scope);
            }
            let existing: bool = query_scalar("SELECT EXISTS(SELECT 1 FROM albert_session_workspace_copies WHERE asset_id=? AND workspace_root=?)")
                .bind(asset).bind(root).fetch_one(&mut **tx).await?;
            if !existing {
                if count >= self.limits.durable_files {
                    return Err(MediaError::Limit);
                }
                count += 1;
            }
            query("INSERT INTO albert_session_workspace_copies(session_id,asset_id,workspace_root) VALUES (?,?,?) ON CONFLICT(asset_id,workspace_root) DO NOTHING")
                .bind(session).bind(asset).bind(root).execute(&mut **tx).await?;
        }
        Ok(())
    }
}
