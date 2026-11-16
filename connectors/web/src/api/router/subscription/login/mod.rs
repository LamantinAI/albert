//! subscription login HTTP resources.
use super::super::input::IgnoredBody;
use axum::{extract::State, Json};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::super::{actor::OwnerActor, host::run, input::resource, input::ApiJson, input::ApiPath};
use crate::api::models::{ApiErrorBody, OpenAiLoginStart, OpenAiLoginSubmission, OpenAiLoginView};
use crate::api::{
    errors::ApiResult, host::HostOperation, host::OpenAiLoginOperation, state::ApiState,
};

pub(super) fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(post_openai_login_attempts))
        .routes(routes!(get_openai_login_attempts_id))
        .routes(routes!(post_openai_login_attempts_id_submissions))
        .routes(routes!(post_openai_login_attempts_id_cancellations))
}

#[utoipa::path(post, operation_id = "subscription_post_openai_login_attempts", path = "/openai-login-attempts", summary = "Create openai login attempts", tag = "subscription",
    request_body = OpenAiLoginStart,
    params(("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = OpenAiLoginView),
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
async fn post_openai_login_attempts(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiJson(input): ApiJson<OpenAiLoginStart>,
) -> ApiResult<Json<OpenAiLoginView>> {
    let operation = { HostOperation::OpenAiLogin(OpenAiLoginOperation::Start(input)) };
    run(&state, &caller, operation).await
}

#[utoipa::path(get, operation_id = "subscription_get_openai_login_attempts_id", path = "/openai-login-attempts/{id}", summary = "Get openai login attempts", tag = "subscription",
    params(("id" = String, Path, description = "Resource identifier")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = OpenAiLoginView),
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
async fn get_openai_login_attempts_id(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<OpenAiLoginView>> {
    let operation = { HostOperation::OpenAiLogin(OpenAiLoginOperation::Get { id: resource(id)? }) };
    run(&state, &caller, operation).await
}

#[utoipa::path(post, operation_id = "subscription_post_openai_login_attempts_id_submissions", path = "/openai-login-attempts/{id}/submissions", summary = "Create openai login attempts submissions", tag = "subscription",
    request_body = OpenAiLoginSubmission,
    params(("id" = String, Path, description = "Resource identifier"), ("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = OpenAiLoginView),
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
async fn post_openai_login_attempts_id_submissions(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiPath(id): ApiPath<String>,
    ApiJson(input): ApiJson<OpenAiLoginSubmission>,
) -> ApiResult<Json<OpenAiLoginView>> {
    let operation = {
        HostOperation::OpenAiLogin(OpenAiLoginOperation::Submit {
            id: resource(id)?,
            input: input,
        })
    };
    run(&state, &caller, operation).await
}

#[utoipa::path(post, operation_id = "subscription_post_openai_login_attempts_id_cancellations", path = "/openai-login-attempts/{id}/cancellations", summary = "Create openai login attempts cancellations", tag = "subscription",
    params(("id" = String, Path, description = "Resource identifier"), ("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = OpenAiLoginView),
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
async fn post_openai_login_attempts_id_cancellations(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiPath(id): ApiPath<String>,
    _body: IgnoredBody,
) -> ApiResult<Json<OpenAiLoginView>> {
    let operation =
        { HostOperation::OpenAiLogin(OpenAiLoginOperation::Cancel { id: resource(id)? }) };
    run(&state, &caller, operation).await
}
