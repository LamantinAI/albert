//! One shared routing handle per Session. Every journal operation holds its
//! backend lease until completion; promotion/end take the exclusive lease.

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use async_trait::async_trait;
use octo_history::{
    AppendReceipt, AppendRequest, ContextWindow, HistoryError, HistoryStore, JournalPage,
    MemoryLimits, PageRequest, RequestAlias, RequestBinding, Result as HistoryResult,
    RetainedMemoryHistory, SessionId, SessionJournal, SessionStore, SqliteHistory, Turn,
};
use rand::random;
use serde_json::to_vec;
use sha2::{Digest, Sha256};
use tokio::sync::{OwnedRwLockReadGuard, RwLock};
use tracing::warn;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Backend {
    Durable,
    Temporary,
    Closed,
}

pub(crate) struct SessionHistory {
    durable: Arc<SqliteHistory>,
    temporary: Arc<RetainedMemoryHistory>,
    handles: RwLock<HashMap<SessionId, Arc<RwLock<Backend>>>>,
    lifecycle: Arc<RwLock<()>>,
    writer_epoch: String,
    sequence: AtomicU64,
}

impl SessionHistory {
    pub fn new(durable: Arc<SqliteHistory>, limits: MemoryLimits) -> HistoryResult<Self> {
        Ok(Self {
            durable,
            temporary: Arc::new(RetainedMemoryHistory::new(limits)?),
            handles: RwLock::new(HashMap::new()),
            lifecycle: Arc::new(RwLock::new(())),
            writer_epoch: format!("{:032x}", random::<u128>()),
            sequence: AtomicU64::new(0),
        })
    }

    pub fn temporary(&self) -> &Arc<RetainedMemoryHistory> {
        &self.temporary
    }

    /// Promotion/end acquire this write lease before the Session backend lease
    /// and SQLite transaction. Native request lookup cannot straddle a move
    /// from RAM to SQL or mistake a discarded receipt for a new request.
    pub fn lifecycle_gate(&self) -> Arc<RwLock<()>> {
        self.lifecycle.clone()
    }

    pub async fn find_request(
        &self,
        namespace: &str,
        request_id: &str,
    ) -> HistoryResult<Option<RequestBinding>> {
        let (binding, _lease) = self.find_request_leased(namespace, request_id).await?;
        Ok(binding)
    }

    pub async fn find_request_leased(
        &self,
        namespace: &str,
        request_id: &str,
    ) -> HistoryResult<(Option<RequestBinding>, OwnedRwLockReadGuard<()>)> {
        let lifecycle = self.lifecycle.clone().read_owned().await;
        let binding = self.find_request_unlocked(namespace, request_id).await?;
        Ok((binding, lifecycle))
    }

    pub async fn find_requests_leased(
        &self,
        requests: &[(String, String)],
    ) -> HistoryResult<(Vec<Option<RequestBinding>>, OwnedRwLockReadGuard<()>)> {
        let lifecycle = self.lifecycle.clone().read_owned().await;
        let mut bindings = Vec::with_capacity(requests.len());
        for (namespace, request_id) in requests {
            bindings.push(self.find_request_unlocked(namespace, request_id).await?);
        }
        Ok((bindings, lifecycle))
    }

    async fn find_request_unlocked(
        &self,
        namespace: &str,
        request_id: &str,
    ) -> HistoryResult<Option<RequestBinding>> {
        let durable = self.durable.find_request(namespace, request_id).await?;
        let temporary = self.temporary.find_request(namespace, request_id).await?;
        let binding = match (durable, temporary) {
            (Some(left), Some(right)) if left != right => {
                return Err(HistoryError::IdempotencyConflict);
            }
            (Some(binding), _) | (_, Some(binding)) => Some(binding),
            (None, None) => None,
        };
        if let Some(binding) = &binding {
            let handle = self.handle(binding.journal.session_id()).await;
            if *handle.read().await == Backend::Closed {
                return Err(HistoryError::SessionNotFound);
            }
        }
        Ok(binding)
    }

    pub async fn append_aliased(
        &self,
        key: &str,
        request: &AppendRequest,
        aliases: &[RequestAlias],
    ) -> HistoryResult<AppendReceipt> {
        let journal = parse_journal(key)?;
        let (lease, store) = self.store(&journal).await?;
        let result = store
            .append_session_aliased(&journal, request, aliases)
            .await;
        drop(lease);
        result
    }

    pub async fn handle(&self, id: &SessionId) -> Arc<RwLock<Backend>> {
        if let Some(handle) = self.handles.read().await.get(id) {
            return handle.clone();
        }
        self.handles
            .write()
            .await
            .entry(id.clone())
            .or_insert_with(|| Arc::new(RwLock::new(Backend::Durable)))
            .clone()
    }

    async fn store(
        &self,
        journal: &SessionJournal,
    ) -> HistoryResult<(OwnedRwLockReadGuard<Backend>, Arc<dyn SessionStore>)> {
        let guard = self.handle(journal.session_id()).await.read_owned().await;
        let store: Arc<dyn SessionStore> = match *guard {
            Backend::Durable => self.durable.clone(),
            Backend::Temporary => self.temporary.clone(),
            Backend::Closed => return Err(HistoryError::SessionNotFound),
        };
        Ok((guard, store))
    }

    pub async fn strict_load(&self, key: &str) -> HistoryResult<Vec<Turn>> {
        let journal = parse_journal(key)?;
        let (lease, store) = self.store(&journal).await?;
        let result = store.load_session(&journal).await;
        drop(lease);
        result
    }
}

/// A child run ID is opaque and can contain slashes. Only the fixed Session UUID
/// prefix is parsed; the complete remaining run ID belongs to that Session.
pub(crate) fn parse_journal(key: &str) -> HistoryResult<SessionJournal> {
    let value = key
        .strip_prefix("session/")
        .ok_or_else(|| HistoryError::InvalidRequest("a session journal key is required".into()))?;
    match value.split_once("/child/") {
        Some((id, run_id)) => {
            let journal = SessionJournal::Child {
                session_id: SessionId::parse(id)?,
                run_id: run_id.into(),
            };
            journal.storage_key()?;
            Ok(journal)
        }
        None => Ok(SessionJournal::Root(SessionId::parse(value)?)),
    }
}

#[async_trait]
impl HistoryStore for SessionHistory {
    fn retains_context(&self) -> bool {
        true
    }

    async fn context(&self, key: &str) -> HistoryResult<ContextWindow> {
        let journal = parse_journal(key)?;
        let (lease, store) = self.store(&journal).await?;
        let result = store.session_context(&journal).await;
        drop(lease);
        result
    }

    async fn save_compact(
        &self,
        key: &str,
        previous_id: Option<i64>,
        through_id: i64,
        content: &str,
    ) -> HistoryResult<bool> {
        let journal = parse_journal(key)?;
        let (lease, store) = self.store(&journal).await?;
        let result = store
            .save_session_compact(&journal, previous_id, through_id, content)
            .await;
        drop(lease);
        result
    }

    /// Legacy trait compatibility only. Runtime decisions use strict_load or
    /// context, whose errors must be surfaced before executing model/tools.
    async fn load(&self, key: &str) -> Vec<Turn> {
        match self.strict_load(key).await {
            Ok(turns) => turns,
            Err(error) => {
                warn!(%error, "legacy history load failed; use strict session loading");
                Vec::new()
            }
        }
    }

    async fn append(&self, key: &str, turns: &[Turn]) -> HistoryResult<()> {
        if turns.is_empty() {
            return Ok(());
        }
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        let request = AppendRequest {
            namespace: "albert.journal".into(),
            idempotency_key: format!("{}:{sequence}", self.writer_epoch),
            digest: format!("{:x}", Sha256::digest(to_vec(turns)?)),
            turns: turns.to_vec(),
        };
        self.append_journal(key, &request).await?;
        Ok(())
    }

    async fn journal_page(&self, key: &str, page: PageRequest) -> HistoryResult<JournalPage> {
        let journal = parse_journal(key)?;
        let (lease, store) = self.store(&journal).await?;
        let result = store.session_page(&journal, page).await;
        drop(lease);
        result
    }

    async fn append_journal(
        &self,
        key: &str,
        request: &AppendRequest,
    ) -> HistoryResult<AppendReceipt> {
        let journal = parse_journal(key)?;
        let (lease, store) = self.store(&journal).await?;
        let result = store.append_session(&journal, request).await;
        drop(lease);
        result
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use octo_history::{
        CreateSession, HistoryStore, MemoryLimits, PageRequest, SessionId, SessionJournal,
        SessionKind, SessionStore, SqliteHistory, Turn,
    };
    use tempfile::tempdir;
    use tokio::{spawn, time::timeout};

    use super::{parse_journal, Backend, SessionHistory};

    #[test]
    fn child_parser_preserves_the_entire_opaque_run_id() {
        let id = SessionId::new();
        let journal = SessionJournal::Child {
            session_id: id,
            run_id: "albert/child-1:abc/child/suffix".into(),
        };
        assert_eq!(
            parse_journal(&journal.storage_key().unwrap()).unwrap(),
            journal
        );
        assert!(parse_journal("42").is_err());
    }

    #[tokio::test]
    async fn retained_routing_keeps_temporary_payload_out_of_sqlite() {
        let directory = tempdir().unwrap();
        let durable = Arc::new(
            SqliteHistory::open_retained(directory.path().join("history.sqlite"))
                .await
                .unwrap(),
        );
        let router = SessionHistory::new(durable.clone(), MemoryLimits::default()).unwrap();
        let mut request = CreateSession::private("owner");
        request.kind = SessionKind::Temporary;
        let session = router.temporary().create_session(request).await.unwrap();
        *router.handle(&session.id).await.write().await = Backend::Temporary;
        let key = SessionJournal::Root(session.id.clone())
            .storage_key()
            .unwrap();
        router
            .append(&key, &[Turn::user("temporary content")])
            .await
            .unwrap();
        assert!(durable.get_session(&session.id).await.unwrap().is_none());
        assert_eq!(
            router.strict_load(&key).await.unwrap()[0].content,
            "temporary content"
        );
        assert_eq!(
            router
                .journal_page(&key, PageRequest::default())
                .await
                .unwrap()
                .records
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn closing_the_shared_handle_fences_a_waiting_writer() {
        let directory = tempdir().unwrap();
        let durable = Arc::new(
            SqliteHistory::open_retained(directory.path().join("history.sqlite"))
                .await
                .unwrap(),
        );
        let session = durable
            .create_session(CreateSession::private("owner"))
            .await
            .unwrap();
        let router = Arc::new(SessionHistory::new(durable, MemoryLimits::default()).unwrap());
        let handle = router.handle(&session.id).await;
        let mut exclusive = handle.write_owned().await;
        let writer = router.clone();
        let key = SessionJournal::Root(session.id).storage_key().unwrap();
        let mut task = spawn(async move { writer.append(&key, &[Turn::assistant("late")]).await });
        assert!(timeout(Duration::from_millis(25), &mut task).await.is_err());
        *exclusive = Backend::Closed;
        drop(exclusive);
        assert!(timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap()
            .is_err());
    }
}
