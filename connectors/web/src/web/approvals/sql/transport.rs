use std::time::Duration;

use octo_approval::{Access, Origin};
use octo_core::{ConnectorContext, ConnectorId, Envelope, EventKind};
use serde_json::Value;

use super::SqlAdapter;
use crate::api::{errors::ApiError, errors::ApiResult};

impl SqlAdapter {
    pub(super) async fn call(
        &self,
        ctx: &ConnectorContext,
        executor: &str,
        kind: &str,
        payload: Value,
        origin: Origin,
        decision: bool,
    ) -> ApiResult<Value> {
        let mut request = Envelope::new(ConnectorId::new("web"), EventKind::new(kind), payload)
            .with_target(ConnectorId::new(executor));
        let ingress = self.runtime.authority.issue(origin, Access::owner());
        request = if decision {
            let proof = self
                .runtime
                .authority
                .bind_decision(&ingress, &request)
                .map_err(|_| ApiError::forbidden())?;
            request.with_context(proof)
        } else {
            let proof = self
                .runtime
                .authority
                .bind(&ingress, &request)
                .map_err(|_| ApiError::forbidden())?;
            request.with_context(proof)
        };
        let timeout = self
            .runtime
            .sessions
            .settings()
            .confirmations
            .native_timeout_ms;
        let expected_kind = format!("{kind}.result");
        let response = ctx
            .publish_and_await_response(request, Duration::from_millis(timeout))
            .await
            .map_err(|_| {
                ApiError::unavailable(
                    "The action outcome is unknown. Check its status before retrying.",
                )
            })?;
        if response.source.as_str() != executor || response.kind.as_str() != expected_kind {
            return Err(ApiError::unavailable(
                "The action status could not be verified.",
            ));
        }
        let value = response
            .payload_as::<Value>()
            .cloned()
            .ok_or_else(ApiError::internal)?;
        if value.get("error").is_some() {
            return Err(ApiError::conflict(
                "The action is unavailable, stale, or no longer authorized.",
            ));
        }
        Ok(value)
    }
}
