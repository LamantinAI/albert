//! sessions starts HTTP resources.
use axum::{extract::State, Json};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::super::{
    actor::BrowserActor, host::run, input::resource, input::ApiJson, input::ApiPath,
};
use crate::api::models::{ApiErrorBody, EmptyObject, SessionStart, SessionStartView};
use crate::api::{errors::ApiResult, host::HostOperation, host::SessionOperation, state::ApiState};

pub(super) fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(post_session_starts))
        .routes(routes!(get_session_starts_id))
        .routes(routes!(post_session_starts_id_cancellations))
}

#[utoipa::path(post, operation_id = "sessions_post_session_starts", path = "/session-starts", summary = "Create session starts", tag = "sessions",
    request_body = SessionStart,
    params(("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = SessionStartView),
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
async fn post_session_starts(
    State(state): State<ApiState>,
    BrowserActor(caller): BrowserActor,
    ApiJson(input): ApiJson<SessionStart>,
) -> ApiResult<Json<SessionStartView>> {
    let operation = { HostOperation::Session(SessionOperation::Start(input)) };
    run(&state, &caller, operation).await
}

#[utoipa::path(get, operation_id = "sessions_get_session_starts_id", path = "/session-starts/{id}", summary = "Get session starts", tag = "sessions",
    params(("id" = String, Path, description = "Resource identifier")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = SessionStartView),
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
async fn get_session_starts_id(
    State(state): State<ApiState>,
    BrowserActor(caller): BrowserActor,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<SessionStartView>> {
    let operation = { HostOperation::Session(SessionOperation::StartGet { id: resource(id)? }) };
    run(&state, &caller, operation).await
}

#[utoipa::path(post, operation_id = "sessions_post_session_starts_id_cancellations", path = "/session-starts/{id}/cancellations", summary = "Create session starts cancellations", tag = "sessions",
    request_body = EmptyObject,
    params(("id" = String, Path, description = "Resource identifier"), ("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = SessionStartView),
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
async fn post_session_starts_id_cancellations(
    State(state): State<ApiState>,
    BrowserActor(caller): BrowserActor,
    ApiPath(id): ApiPath<String>,
    ApiJson(_input): ApiJson<EmptyObject>,
) -> ApiResult<Json<SessionStartView>> {
    let operation = { HostOperation::Session(SessionOperation::StartCancel { id: resource(id)? }) };
    run(&state, &caller, operation).await
}
