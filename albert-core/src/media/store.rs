use std::{collections::HashMap, sync::Arc, time::Duration, time::Instant};

use octo_core::Blob;
use octo_history::SessionId;
use sqlx::{query, query_as, query_scalar, Sqlite, SqlitePool, Transaction};
use tokio::sync::Mutex;

use super::{types::content_type, types::filename, Asset, MediaError, MediaLimits, MediaResult};

#[derive(Clone)]
struct Entry {
    asset: Asset,
    blob: Blob,
    owner: String,
    session: Option<String>,
    expires: Instant,
}
#[derive(Clone)]
pub struct PreparedMedia {
    entries: Vec<Entry>,
}
impl PreparedMedia {
    pub fn assets(&self) -> Vec<Asset> {
        self.entries.iter().map(|e| e.asset.clone()).collect()
    }
    pub fn blobs(&self) -> Vec<Blob> {
        self.entries.iter().map(|e| e.blob.clone()).collect()
    }
}

pub struct MediaStore {
    pool: SqlitePool,
    pub limits: MediaLimits,
    entries: Mutex<HashMap<String, Entry>>,
    pub(crate) temporary_writes: Mutex<()>,
}
impl MediaStore {
    pub fn new(pool: SqlitePool, limits: MediaLimits) -> Arc<Self> {
        Arc::new(Self {
            pool,
            limits,
            entries: Mutex::new(HashMap::new()),
            temporary_writes: Mutex::new(()),
        })
    }
    pub async fn stage(
        &self,
        owner: &str,
        bytes: Vec<u8>,
        mime: &str,
        name: &str,
    ) -> MediaResult<Asset> {
        if bytes.is_empty() || bytes.len() > self.limits.max_file_bytes {
            return Err(MediaError::Limit);
        }
        let content_type = content_type(&bytes, mime)?;
        let asset = Asset {
            id: SessionId::new().to_string(),
            filename: filename(name),
            content_type,
            size: bytes.len(),
        };
        let mut entries = self.entries.lock().await;
        entries.retain(|_, e| e.session.is_some() || e.expires > Instant::now());
        let total: usize = entries.values().map(|e| e.asset.size).sum();
        let account: usize = entries
            .values()
            .filter(|e| e.owner == owner && e.session.is_none())
            .map(|e| e.asset.size)
            .sum();
        let account_files = entries
            .values()
            .filter(|e| e.owner == owner && e.session.is_none())
            .count();
        if entries.len() >= self.limits.memory_files
            || account_files >= self.limits.staged_account_files
            || total.saturating_add(bytes.len()) > self.limits.memory_bytes
            || account.saturating_add(bytes.len()) > self.limits.staged_account_bytes
        {
            return Err(MediaError::Limit);
        }
        let blob = Blob::new(bytes, &asset.content_type).with_filename(&asset.filename);
        entries.insert(
            asset.id.clone(),
            Entry {
                asset: asset.clone(),
                blob,
                owner: owner.into(),
                session: None,
                expires: Instant::now() + Duration::from_secs(self.limits.staged_ttl_seconds),
            },
        );
        Ok(asset)
    }
    pub async fn remove_staged(&self, owner: &str, id: &str) -> MediaResult<()> {
        let mut entries = self.entries.lock().await;
        if entries
            .get(id)
            .is_some_and(|e| e.owner == owner && e.session.is_none())
        {
            entries.remove(id);
            Ok(())
        } else {
            Err(MediaError::NotFound)
        }
    }
    pub async fn prepare(
        &self,
        owner: &str,
        session: Option<&str>,
        ids: &[String],
    ) -> MediaResult<PreparedMedia> {
        if ids.len() > self.limits.max_attachments {
            return Err(MediaError::Limit);
        }
        let cached = {
            let entries = self.entries.lock().await;
            ids.iter()
                .filter_map(|id| entries.get(id).cloned().map(|e| (id.clone(), e)))
                .collect::<HashMap<_, _>>()
        };
        let mut selected = Vec::new();
        for id in ids {
            if selected.iter().any(|e: &Entry| e.asset.id == *id) {
                return Err(MediaError::Scope);
            }
            if let Some(e) = cached
                .get(id)
                .filter(|e| e.owner == owner && (e.session.is_some() || e.expires > Instant::now()))
            {
                if e.session
                    .as_deref()
                    .is_some_and(|bound| Some(bound) != session)
                {
                    return Err(MediaError::Scope);
                }
                selected.push(e.clone());
            } else {
                // The caller authorizes and leases this session before loading
                // accepted input. Staging is not a durable acceptance receipt.
                let session = session.ok_or(MediaError::NotFound)?;
                let asset = self.metadata(session, id, false).await?;
                if asset.size > self.limits.max_file_bytes {
                    return Err(MediaError::Limit);
                }
                let bytes = self.slice(session, id, false, 0, asset.size).await?;
                let blob = Blob::new(bytes, &asset.content_type).with_filename(&asset.filename);
                selected.push(Entry {
                    asset,
                    blob,
                    owner: owner.into(),
                    session: Some(session.into()),
                    expires: Instant::now(),
                });
            }
        }
        if selected
            .iter()
            .filter(|e| e.asset.content_type.starts_with("audio/"))
            .count()
            > 1
        {
            return Err(MediaError::Limit);
        }
        Ok(PreparedMedia { entries: selected })
    }
    /// Call while holding the session backend lease, before appending a RAM
    /// journal. Reservations remain retryable if the journal rejects the append.
    pub async fn bind_temporary(
        &self,
        session: &str,
        assets: &[Asset],
    ) -> MediaResult<Vec<String>> {
        self.reserve(session, assets, false).await
    }
    pub async fn reserve_durable(
        &self,
        session: &str,
        assets: &[Asset],
    ) -> MediaResult<Vec<String>> {
        self.reserve(session, assets, true).await
    }
    async fn reserve(
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
    pub async fn rollback_temporary(&self, session: &str, ids: &[String]) {
        let mut entries = self.entries.lock().await;
        for id in ids {
            if let Some(e) = entries
                .get_mut(id)
                .filter(|e| e.session.as_deref() == Some(session))
            {
                e.session = None;
            }
        }
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
    pub async fn discard_session(&self, session: &str) {
        self.entries
            .lock()
            .await
            .retain(|_, e| e.session.as_deref() != Some(session));
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
