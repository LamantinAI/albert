//! sessions catalog HTTP resources.
use axum::{extract::State, Json};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::super::{
    actor::BrowserActor, host::run, input::page, input::resource, input::ApiJson, input::ApiPath,
    input::ApiQuery,
};
use crate::api::models::{
    ApiErrorBody, ForkCreate, PageQuery, SessionCreate, SessionList, SessionUpdate, SessionView,
};
use crate::api::{errors::ApiResult, host::HostOperation, host::SessionOperation, state::ApiState};

pub(super) fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(get_sessions))
        .routes(routes!(post_sessions))
        .routes(routes!(post_session_fork))
        .routes(routes!(get_sessions_id))
        .routes(routes!(patch_sessions_id))
}

#[utoipa::path(get, operation_id = "sessions_get_sessions", path = "/sessions", summary = "Get sessions", tag = "sessions",
    params(PageQuery),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = SessionList),
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
async fn get_sessions(
    State(state): State<ApiState>,
    BrowserActor(caller): BrowserActor,
    ApiQuery(query): ApiQuery<PageQuery>,
) -> ApiResult<Json<SessionList>> {
    let operation = HostOperation::Session(SessionOperation::List(page(query)?));
    run(&state, &caller, operation).await
}

#[utoipa::path(post, operation_id = "sessions_post_sessions", path = "/sessions", summary = "Create sessions", tag = "sessions",
    request_body = SessionCreate,
    params(("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = SessionView),
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
async fn post_sessions(
    State(state): State<ApiState>,
    BrowserActor(caller): BrowserActor,
    ApiJson(input): ApiJson<SessionCreate>,
) -> ApiResult<Json<SessionView>> {
    let operation = HostOperation::Session(SessionOperation::Create(input));
    run(&state, &caller, operation).await
}

#[utoipa::path(post, operation_id = "sessions_post_sessions_id_forks", path = "/sessions/{id}/forks",
    summary = "Fork a saved private conversation", tag = "sessions", request_body = ForkCreate,
    params(("id" = String, Path), ("x-albert-csrf" = String, Header)),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Forked conversation", body = SessionView),
        (status = 400, description = "Invalid request", body = ApiErrorBody),
        (status = 401, description = "Sign-in required", body = ApiErrorBody),
        (status = 403, description = "Insufficient authority", body = ApiErrorBody),
        (status = 404, description = "Resource unavailable", body = ApiErrorBody),
        (status = 409, description = "Conflicting state", body = ApiErrorBody))
)]
async fn post_session_fork(
    State(state): State<ApiState>,
    BrowserActor(caller): BrowserActor,
    ApiPath(id): ApiPath<String>,
    ApiJson(input): ApiJson<ForkCreate>,
) -> ApiResult<Json<SessionView>> {
    run(
        &state,
        &caller,
        HostOperation::Session(SessionOperation::Fork {
            id: resource(id)?,
            input,
        }),
    )
    .await
}

#[utoipa::path(get, operation_id = "sessions_get_sessions_id", path = "/sessions/{id}", summary = "Get sessions", tag = "sessions",
    params(("id" = String, Path, description = "Resource identifier")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = SessionView),
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
async fn get_sessions_id(
    State(state): State<ApiState>,
    BrowserActor(caller): BrowserActor,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<SessionView>> {
    let operation = { HostOperation::Session(SessionOperation::Get { id: resource(id)? }) };
    run(&state, &caller, operation).await
}

#[utoipa::path(patch, operation_id = "sessions_patch_sessions_id", path = "/sessions/{id}", summary = "Update sessions", tag = "sessions",
    request_body = SessionUpdate,
    params(("id" = String, Path, description = "Resource identifier"), ("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = SessionView),
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
async fn patch_sessions_id(
    State(state): State<ApiState>,
    BrowserActor(caller): BrowserActor,
    ApiPath(id): ApiPath<String>,
    ApiJson(input): ApiJson<SessionUpdate>,
) -> ApiResult<Json<SessionView>> {
    let operation = HostOperation::Session(SessionOperation::Update {
        id: resource(id)?,
        input: input,
    });
    run(&state, &caller, operation).await
}
