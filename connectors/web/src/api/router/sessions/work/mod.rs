//! sessions work HTTP resources.
use axum::{extract::State, Json};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::super::{
    actor::BrowserActor, host::run, input::resource, input::ApiJson, input::ApiPath,
};
use crate::api::models::{
    ApiErrorBody, AttemptView, CancelRequest, CompactCreate, ComposerView, MessageReceipt,
    SessionState,
};
use crate::api::{errors::ApiResult, host::HostOperation, host::SessionOperation, state::ApiState};

pub(super) fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(get_composer))
        .routes(routes!(post_sessions_id_compactions))
        .routes(routes!(get_sessions_id_state))
        .routes(routes!(post_attempts_id_cancellations))
}

#[utoipa::path(get, operation_id = "sessions_get_composer", path = "/composer", summary = "Get composer", tag = "sessions",
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = ComposerView),
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
async fn get_composer(
    State(state): State<ApiState>,
    BrowserActor(caller): BrowserActor,
) -> ApiResult<Json<ComposerView>> {
    let operation = HostOperation::Session(SessionOperation::Composer);
    run(&state, &caller, operation).await
}

#[utoipa::path(post, operation_id = "sessions_post_sessions_id_compactions", path = "/sessions/{id}/compactions", summary = "Create sessions compactions", tag = "sessions",
    request_body = CompactCreate,
    params(("id" = String, Path, description = "Resource identifier"), ("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = MessageReceipt),
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
async fn post_sessions_id_compactions(
    State(state): State<ApiState>,
    BrowserActor(caller): BrowserActor,
    ApiPath(id): ApiPath<String>,
    ApiJson(input): ApiJson<CompactCreate>,
) -> ApiResult<Json<MessageReceipt>> {
    let operation = {
        HostOperation::Session(SessionOperation::Compact {
            id: resource(id)?,
            input: input,
        })
    };
    run(&state, &caller, operation).await
}

#[utoipa::path(get, operation_id = "sessions_get_sessions_id_state", path = "/sessions/{id}/state", summary = "Get sessions state", tag = "sessions",
    params(("id" = String, Path, description = "Resource identifier")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = SessionState),
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
async fn get_sessions_id_state(
    State(state): State<ApiState>,
    BrowserActor(caller): BrowserActor,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<SessionState>> {
    let operation = { HostOperation::Session(SessionOperation::State { id: resource(id)? }) };
    run(&state, &caller, operation).await
}

#[utoipa::path(post, operation_id = "sessions_post_attempts_id_cancellations", path = "/attempts/{id}/cancellations", summary = "Create attempts cancellations", tag = "sessions",
    request_body = CancelRequest,
    params(("id" = String, Path, description = "Resource identifier"), ("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = AttemptView),
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
async fn post_attempts_id_cancellations(
    State(state): State<ApiState>,
    BrowserActor(caller): BrowserActor,
    ApiPath(id): ApiPath<String>,
    ApiJson(input): ApiJson<CancelRequest>,
) -> ApiResult<Json<AttemptView>> {
    let operation = {
        HostOperation::Session(SessionOperation::Cancel {
            attempt_id: resource(id)?,
            input: input,
        })
    };
    run(&state, &caller, operation).await
}
