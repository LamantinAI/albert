use super::{parse_journal, Backend, SessionHistory};
use crate::media::format_assets;
use octo_history::{
    AppendReceipt, AppendRequest, HistoryError, RequestAlias, RequestBinding,
    Result as HistoryResult, SessionStore,
};
use tokio::sync::OwnedRwLockReadGuard;
impl SessionHistory {
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
    pub(in crate::history) async fn find_request_unlocked(
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
        let assets = format_assets(&request.turns);
        let result = if assets.is_empty() {
            store
                .append_session_aliased(&journal, request, aliases)
                .await
        } else if *lease == Backend::Durable {
            self.append_media_durable(&journal, request, aliases, &assets)
                .await
        } else {
            let _media_gate = self.media.temporary_writes.lock().await;
            let reserved = self
                .media
                .bind_temporary(journal.session_id().as_str(), &assets)
                .await
                .map_err(|e| HistoryError::InvalidRequest(e.to_string()))?;
            let result = store
                .append_session_aliased(&journal, request, aliases)
                .await;
            if result.is_err() {
                self.media
                    .rollback_temporary(journal.session_id().as_str(), &reserved)
                    .await;
            }
            result
        };
        drop(lease);
        result
    }
}
