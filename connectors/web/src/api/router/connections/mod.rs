//! connections HTTP resources.
use axum::{extract::State, Json};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::{actor::BrowserActor, host::run};
use crate::api::models::{ApiErrorBody, ConnectorList};
use crate::api::{errors::ApiResult, host::HostOperation, state::ApiState};

pub(super) fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new().routes(routes!(get_connectors))
}

#[utoipa::path(get, operation_id = "connections_get_connectors", path = "/connectors", summary = "Get connectors", tag = "connections",
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = ConnectorList),
        (status = 400, description = "Invalid request", body = ApiErrorBody),
        (status = 401, description = "Sign-in required", body = ApiErrorBody),
        (status = 403, description = "Insufficient authority", body = ApiErrorBody),
        (status = 404, description = "Resource unavailable", body = ApiErrorBody),
        (status = 409, description = "Conflicting state", body = ApiErrorBody),
        (status = 412, description = "Revision changed", body = ApiErrorBody),
        (status = 413, description = "Body too large", body = ApiErrorBody),
        (status = 503, description = "Service unavailable", body = ApiErrorBody),
        (status = 500, description = "Internal failure", body = ApiErrorBody))
)]
async fn get_connectors(
    State(state): State<ApiState>,
    BrowserActor(caller): BrowserActor,
) -> ApiResult<Json<ConnectorList>> {
    let operation = HostOperation::Connectors;
    run(&state, &caller, operation).await
}
