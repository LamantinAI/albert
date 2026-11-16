mod projection;
mod transport;

use std::sync::Arc;

use albert_core::runtime::AgentRuntime;
use async_trait::async_trait;
use octo_approval::ApprovalPresentation;
use octo_core::{ConnectorContext, Envelope};
use serde_json::json;

use super::native::{ApprovalAdapter, ApprovalOwner, NativeRecord};
use crate::api::{errors::ApiError, errors::ApiResult};

pub(super) struct SqlAdapter {
    runtime: Arc<AgentRuntime>,
}
impl SqlAdapter {
    pub fn new(runtime: Arc<AgentRuntime>) -> Self {
        Self { runtime }
    }
    fn targets(&self) -> Vec<String> {
        self.runtime
            .context()
            .map(|ctx| {
                ctx.connectors()
                    .iter()
                    .filter(|entry| {
                        entry
                            .capabilities
                            .event_kinds_accept
                            .iter()
                            .any(|kind| kind.as_str() == "sql.approval.list")
                    })
                    .map(|entry| entry.id.to_string())
                    .collect()
            })
            .unwrap_or_default()
    }
    fn target(&self, executor: &str) -> ApiResult<()> {
        self.targets()
            .iter()
            .any(|id| id == executor)
            .then_some(())
            .ok_or_else(ApiError::not_found)
    }
}
#[async_trait]
impl ApprovalAdapter for SqlAdapter {
    fn id(&self) -> &'static str {
        "sql"
    }
    fn observe(&self, event: &Envelope) -> Option<NativeRecord> {
        let presentation = event.context.get::<ApprovalPresentation>()?;
        if event.kind.as_str() != "chat.reply"
            || event.source.as_str() != presentation.executor
            || event.target.as_ref().is_none_or(|target| {
                self.runtime
                    .context()
                    .is_none_or(|ctx| !ctx.connectors().iter().any(|entry| entry.id == *target))
            })
            || event.channel.as_ref().map(|id| id.as_str())
                != Some(presentation.private_destination.as_str())
            || presentation.decision_kind != "sql.approval.decide"
            || presentation.status_kind != "sql.operation.status"
            || self.target(&presentation.executor).is_err()
        {
            return None;
        }
        Some(Self::notice(presentation, event.target.as_ref()?.as_str()))
    }
    async fn list(
        &self,
        ctx: &ConnectorContext,
        owner: &ApprovalOwner,
    ) -> ApiResult<Vec<NativeRecord>> {
        let mut records = Vec::new();
        for executor in self.targets() {
            let result = self
                .call(
                    ctx,
                    &executor,
                    "sql.approval.list",
                    json!({}),
                    owner.origin.clone(),
                    false,
                )
                .await?;
            for item in result["items"].as_array().into_iter().flatten() {
                records.push(Self::project(&executor, &owner.origin.principal, item)?);
            }
        }
        Ok(records)
    }
    async fn get(
        &self,
        ctx: &ConnectorContext,
        owner: &ApprovalOwner,
        executor: &str,
        operation: &str,
        details: bool,
    ) -> ApiResult<NativeRecord> {
        self.target(executor)?;
        let result = self
            .call(
                ctx,
                executor,
                "sql.operation.status",
                json!({"operation_id":operation,"include_approval":true,"include_details":details}),
                owner.origin.clone(),
                false,
            )
            .await?;
        Self::project(executor, &owner.origin.principal, &result)
    }
    async fn decide(
        &self,
        ctx: &ConnectorContext,
        owner: &ApprovalOwner,
        record: &NativeRecord,
        approve: bool,
    ) -> ApiResult<NativeRecord> {
        self.target(&record.executor)?;
        if record.recipient_principal != owner.origin.principal {
            return Err(ApiError::forbidden());
        }
        let mut origin = owner.origin.clone();
        origin.connector = record.decision_connector.clone();
        origin.conversation = record.private_destination.clone();
        origin.private_destination = record.private_destination.clone();
        self.call(
            ctx,
            &record.executor,
            "sql.approval.decide",
            json!({"operation_id":record.operation,"approve":approve}),
            origin,
            true,
        )
        .await?;
        self.get(ctx, owner, &record.executor, &record.operation, false)
            .await
    }
}
