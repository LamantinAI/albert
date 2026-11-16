use super::MediaStore;
use crate::media::{Asset, MediaError, MediaResult};
use sqlx::{query, query_as, query_scalar, Sqlite, Transaction};
impl MediaStore {
    pub async fn reserve_durable(
        &self,
        session: &str,
        assets: &[Asset],
    ) -> MediaResult<Vec<String>> {
        self.reserve(session, assets, true).await
    }
    pub(in crate::media) async fn reserve(
        &self,
        session: &str,
        assets: &[Asset],
        allow_persisted: bool,
    ) -> MediaResult<Vec<String>> {
        let mut entries = self.entries.lock().await;
        for asset in assets {
            match entries.get(&asset.id) {
                Some(e)
                    if e.asset != *asset || e.session.as_deref().is_some_and(|s| s != session) =>
                {
                    return Err(MediaError::Scope)
                }
                None if !allow_persisted => return Err(MediaError::NotFound),
                _ => {}
            }
        }
        let mut reserved = Vec::new();
        for asset in assets {
            if let Some(e) = entries.get_mut(&asset.id) {
                if e.session.is_none() {
                    e.session = Some(session.into());
                    reserved.push(asset.id.clone());
                }
            }
        }
        Ok(reserved)
    }
    /// The asset row and journal reference commit atomically in the caller's
    /// transaction. Staging is retained until acknowledgement, so rollback and
    /// uncertain commits are safe to retry.
    pub async fn persist_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        session: &str,
        assets: &[Asset],
    ) -> MediaResult<()> {
        let entries = self.entries.lock().await;
        let selected: Vec<_> = assets
            .iter()
            .map(|a| (a.clone(), entries.get(&a.id).cloned()))
            .collect();
        drop(entries);
        for (asset, entry) in selected {
            let stored: Option<String> =
                query_scalar("SELECT session_id FROM albert_media_assets WHERE id=?")
                    .bind(&asset.id)
                    .fetch_optional(&mut **tx)
                    .await?;
            if let Some(bound) = stored {
                if bound != session {
                    return Err(MediaError::Scope);
                }
                continue;
            }
            let e = entry.ok_or(MediaError::NotFound)?;
            if e.asset != asset || e.session.as_deref() != Some(session) {
                return Err(MediaError::Scope);
            }
            let (total, count): (i64, i64) =
                query_as("SELECT COALESCE(SUM(size),0),COUNT(*) FROM albert_media_assets")
                    .fetch_one(&mut **tx)
                    .await?;
            if count >= self.limits.durable_files
                || total.saturating_add(asset.size as i64) > self.limits.durable_bytes
            {
                return Err(MediaError::Limit);
            }
            query("INSERT INTO albert_media_assets(id,session_id,filename,content_type,size,bytes) VALUES(?,?,?,?,?,?)")
                .bind(&asset.id).bind(session).bind(&asset.filename).bind(&asset.content_type).bind(asset.size as i64).bind(e.blob.bytes().as_ref()).execute(&mut **tx).await?;
        }
        Ok(())
    }
    pub async fn promote_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        session: &str,
    ) -> MediaResult<()> {
        let assets: Vec<_> = self
            .entries
            .lock()
            .await
            .values()
            .filter(|e| e.session.as_deref() == Some(session))
            .map(|e| e.asset.clone())
            .collect();
        self.persist_in(tx, session, &assets).await
    }
    pub async fn release_durable(&self, assets: &[Asset]) {
        let mut entries = self.entries.lock().await;
        for asset in assets {
            entries.remove(&asset.id);
        }
    }
    /// The caller must authorize the session and hold its backend lease.
    pub async fn metadata(&self, session: &str, id: &str, temporary: bool) -> MediaResult<Asset> {
        if temporary {
            return self
                .entries
                .lock()
                .await
                .get(id)
                .filter(|e| e.session.as_deref() == Some(session))
                .map(|e| e.asset.clone())
                .ok_or(MediaError::NotFound);
        }
        let row: Option<(String,String,i64)> = query_as("SELECT filename,content_type,size FROM albert_media_assets WHERE id=? AND session_id=?").bind(id).bind(session).fetch_optional(&self.pool).await?;
        row.map(|(filename, content_type, size)| Asset {
            id: id.into(),
            filename,
            content_type,
            size: size as usize,
        })
        .ok_or(MediaError::NotFound)
    }
    pub async fn slice(
        &self,
        session: &str,
        id: &str,
        temporary: bool,
        offset: usize,
        len: usize,
    ) -> MediaResult<Vec<u8>> {
        if temporary {
            let entries = self.entries.lock().await;
            return entries
                .get(id)
                .filter(|e| e.session.as_deref() == Some(session))
                .and_then(|e| e.blob.bytes().get(offset..offset.checked_add(len)?))
                .map(<[u8]>::to_vec)
                .ok_or(MediaError::NotFound);
        }
        query_scalar(
            "SELECT substr(bytes,?,?) FROM albert_media_assets WHERE id=? AND session_id=?",
        )
        .bind(offset as i64 + 1)
        .bind(len as i64)
        .bind(id)
        .bind(session)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(MediaError::NotFound)
    }
}
