use std::sync::Arc;

use async_trait::async_trait;
use octo_approval::{Origin, ScopeBinding};
use octo_core::{ConnectorContext, Envelope};
use serde_json::Value;

use super::handle::ApprovalHandle;
use crate::api::{errors::ApiResult, models::ApprovalState};

#[derive(Clone)]
pub(super) struct ApprovalOwner {
    pub account: String,
    pub user: String,
    pub origin: Origin,
}
#[derive(Clone)]
pub(super) struct NativeRecord {
    pub adapter: String,
    pub executor: String,
    pub operation: String,
    pub operation_hash: String,
    pub recipient_principal: String,
    pub origin_scope: Option<ScopeBinding>,
    pub decision_connector: String,
    pub private_destination: String,
    pub state: ApprovalState,
    pub title: String,
    pub action: String,
    pub effects: Vec<String>,
    pub details: Option<Value>,
    pub expires_at: i64,
    pub can_decide: bool,
}
impl NativeRecord {
    pub fn handle(&self) -> ApprovalHandle {
        ApprovalHandle::Native {
            adapter: self.adapter.clone(),
            executor: self.executor.clone(),
            operation: self.operation.clone(),
        }
    }
}
/// Native execution policy stays inside each domain adapter and its executor.
#[async_trait]
pub(super) trait ApprovalAdapter: Send + Sync {
    fn id(&self) -> &'static str;
    fn observe(&self, event: &Envelope) -> Option<NativeRecord>;
    async fn list(
        &self,
        ctx: &ConnectorContext,
        owner: &ApprovalOwner,
    ) -> ApiResult<Vec<NativeRecord>>;
    async fn get(
        &self,
        ctx: &ConnectorContext,
        owner: &ApprovalOwner,
        executor: &str,
        operation: &str,
        details: bool,
    ) -> ApiResult<NativeRecord>;
    async fn decide(
        &self,
        ctx: &ConnectorContext,
        owner: &ApprovalOwner,
        record: &NativeRecord,
        approve: bool,
    ) -> ApiResult<NativeRecord>;
}
pub(super) type NativeAdapters = Vec<Arc<dyn ApprovalAdapter>>;
