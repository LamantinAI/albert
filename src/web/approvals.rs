use std::{
    sync::{Arc, RwLock},
    time::Duration,
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use octo_approval::{Access, Origin};
use octo_core::{ConnectorContext, ConnectorId, Envelope, EventKind};
use serde_json::{from_slice, json, to_vec, Value};

use super::runtime::AgentRuntime;
use crate::{
    api::{
        errors::{ApiError, ApiResult},
        host::{ApprovalOperation, HostResponse},
        models::{ApprovalList, ApprovalView},
        principal::Principal,
    },
    sessions::UserId,
};

pub(crate) struct ApprovalService {
    runtime: Arc<AgentRuntime>,
    context: RwLock<Option<ConnectorContext>>,
}
impl ApprovalService {
    pub fn new(runtime: Arc<AgentRuntime>) -> Self {
        Self {
            runtime,
            context: RwLock::new(None),
        }
    }
    pub fn attach(&self, context: ConnectorContext) {
        *self.context.write().unwrap() = Some(context);
    }
    pub async fn observe(&self, _event: &Envelope) {}

    async fn call(
        &self,
        connector: &str,
        kind: &str,
        payload: Value,
        origin: Origin,
        decision: bool,
    ) -> ApiResult<Value> {
        let context = self
            .context
            .read()
            .unwrap()
            .clone()
            .ok_or_else(|| ApiError::unavailable("Transport is starting."))?;
        let mut request = Envelope::new(ConnectorId::new("web"), EventKind::new(kind), payload)
            .with_target(ConnectorId::new(connector));
        let ingress = self.runtime.authority.issue(origin, Access::owner());
        if decision {
            let proof = self
                .runtime
                .authority
                .bind_decision(&ingress, &request)
                .map_err(|_| ApiError::forbidden())?;
            request = request.with_context(proof);
        } else {
            let proof = self
                .runtime
                .authority
                .bind(&ingress, &request)
                .map_err(|_| ApiError::forbidden())?;
            request = request.with_context(proof);
        }
        let result = context
            .publish_and_await_response(request, Duration::from_secs(10))
            .await
            .map_err(|_| {
                ApiError::unavailable("Approval outcome is unknown. Refresh before retrying.")
            })?;
        let value = result
            .payload_as::<Value>()
            .cloned()
            .ok_or_else(ApiError::internal)?;
        if value.get("error").is_some() {
            return Err(ApiError::conflict(
                "The operation is unavailable, stale, or no longer authorized.",
            ));
        }
        Ok(value)
    }

    async fn pending(&self, caller: &Principal) -> ApiResult<(Origin, Vec<(String, Value)>)> {
        caller.owner_permit()?;
        let user = UserId::parse(caller.canonical_user_id().ok_or_else(ApiError::forbidden)?)
            .map_err(|_| ApiError::forbidden())?;
        let principal = self
            .runtime
            .sessions
            .web_sql_principal(&user)
            .await
            .map_err(|_| ApiError::forbidden())?;
        let destination = format!("user:{}", user.as_str());
        let origin = Origin {
            principal,
            connector: "web".into(),
            conversation: destination.clone(),
            private_destination: destination,
            may_approve: true,
        };
        let ctx = self
            .runtime
            .context()
            .ok_or_else(|| ApiError::unavailable("Runtime is starting."))?;
        let mut all = Vec::new();
        for connector in ctx.connectors().iter().filter(|entry| {
            entry
                .capabilities
                .event_kinds_accept
                .iter()
                .any(|kind| kind.as_str() == "sql.approval.list")
        }) {
            let value = self
                .call(
                    connector.id.as_str(),
                    "sql.approval.list",
                    json!({}),
                    origin.clone(),
                    false,
                )
                .await?;
            for entry in value["items"].as_array().into_iter().flatten() {
                all.push((connector.id.to_string(), entry.clone()));
            }
        }
        Ok((origin, all))
    }

    pub async fn operation(
        &self,
        caller: &Principal,
        operation: ApprovalOperation,
    ) -> ApiResult<HostResponse> {
        let (origin, pending) = self.pending(caller).await?;
        match operation {
            ApprovalOperation::List => Ok(HostResponse::Approvals(ApprovalList {
                items: pending
                    .into_iter()
                    .map(|(connector, item)| view(connector, item))
                    .collect(),
            })),
            ApprovalOperation::Decide { id, input } => {
                let bytes = URL_SAFE_NO_PAD
                    .decode(&id)
                    .map_err(|_| ApiError::not_found())?;
                let (executor, operation): (String, String) =
                    from_slice(&bytes).map_err(|_| ApiError::not_found())?;
                let (_, item) = pending
                    .iter()
                    .find(|(connector, item)| {
                        connector == &executor
                            && item["operation_id"].as_str() == Some(operation.as_str())
                    })
                    .ok_or_else(ApiError::not_found)?;
                let connector = item["decision_connector"]
                    .as_str()
                    .ok_or_else(ApiError::internal)?
                    .to_owned();
                let private = item["private_destination"]
                    .as_str()
                    .ok_or_else(ApiError::internal)?
                    .to_owned();
                let mut human = origin;
                human.connector = connector;
                human.conversation = private.clone();
                human.private_destination = private;
                let result = self
                    .call(
                        &executor,
                        "sql.approval.decide",
                        json!({"operation_id":operation,"approve":input.approve}),
                        human,
                        true,
                    )
                    .await?;
                let mut shown = view(executor, item.clone());
                shown.state = result["status"].as_str().unwrap_or("unknown").into();
                Ok(HostResponse::Approval(shown))
            }
        }
    }
}

fn view(connector: String, item: Value) -> ApprovalView {
    let operation = item["operation_id"].as_str().unwrap_or("");
    ApprovalView {
        id: URL_SAFE_NO_PAD.encode(to_vec(&(&connector, operation)).expect("IDs serialize")),
        connector,
        state: item["status"].as_str().unwrap_or("unknown").into(),
        summary: item["summary"].clone(),
        expires_at: item["expires_at"].as_i64(),
    }
}
