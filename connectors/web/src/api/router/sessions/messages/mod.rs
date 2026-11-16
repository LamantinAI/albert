//! sessions messages HTTP resources.
use axum::{extract::State, Json};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::super::{
    actor::BrowserActor, host::run, input::page, input::resource, input::ApiJson, input::ApiPath,
    input::ApiQuery,
};
use crate::api::models::{ApiErrorBody, MessageCreate, MessageList, MessageReceipt, PageQuery};
use crate::api::{errors::ApiResult, host::HostOperation, host::SessionOperation, state::ApiState};

pub(super) fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(get_sessions_id_messages))
        .routes(routes!(post_sessions_id_messages))
        .routes(routes!(get_sessions_id_messages_request_id))
}

#[utoipa::path(get, operation_id = "sessions_get_sessions_id_messages", path = "/sessions/{id}/messages", summary = "Get sessions messages", tag = "sessions",
    params(("id" = String, Path, description = "Resource identifier"), PageQuery),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = MessageList),
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
async fn get_sessions_id_messages(
    State(state): State<ApiState>,
    BrowserActor(caller): BrowserActor,
    ApiPath(id): ApiPath<String>,
    ApiQuery(query): ApiQuery<PageQuery>,
) -> ApiResult<Json<MessageList>> {
    let operation = HostOperation::Session(SessionOperation::Messages {
        id: resource(id)?,
        page: page(query)?,
    });
    run(&state, &caller, operation).await
}

#[utoipa::path(post, operation_id = "sessions_post_sessions_id_messages", path = "/sessions/{id}/messages", summary = "Create sessions messages", tag = "sessions",
    request_body = MessageCreate,
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
async fn post_sessions_id_messages(
    State(state): State<ApiState>,
    BrowserActor(caller): BrowserActor,
    ApiPath(id): ApiPath<String>,
    ApiJson(input): ApiJson<MessageCreate>,
) -> ApiResult<Json<MessageReceipt>> {
    let operation = HostOperation::Session(SessionOperation::Send {
        id: resource(id)?,
        input: input,
    });
    run(&state, &caller, operation).await
}

#[utoipa::path(get, operation_id = "sessions_get_sessions_id_messages_request_id", path = "/sessions/{id}/messages/{request_id}", summary = "Get sessions messages receipt", tag = "sessions",
    params(("id" = String, Path, description = "Resource identifier"), ("request_id" = String, Path, description = "Resource identifier")),
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
async fn get_sessions_id_messages_request_id(
    State(state): State<ApiState>,
    BrowserActor(caller): BrowserActor,
    ApiPath((id, request_id)): ApiPath<(String, String)>,
) -> ApiResult<Json<MessageReceipt>> {
    let operation = {
        HostOperation::Session(SessionOperation::Receipt {
            id: resource(id)?,
            request_id: resource(request_id)?,
        })
    };
    run(&state, &caller, operation).await
}
