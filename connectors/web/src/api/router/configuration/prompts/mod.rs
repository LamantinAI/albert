//! configuration prompts HTTP resources.
use axum::{extract::State, Json};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::super::{actor::OwnerActor, host::run, input::resource, input::ApiJson, input::ApiPath};
use crate::api::models::{ApiErrorBody, ConfigDocument, ConfigWrite};
use crate::api::{errors::ApiResult, host::ConfigOperation, host::HostOperation, state::ApiState};

pub(super) fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(get_prompts_id))
        .routes(routes!(put_prompts_id))
}

#[utoipa::path(get, operation_id = "configuration_get_prompts_id", path = "/prompts/{id}", summary = "Get prompts", tag = "configuration",
    params(("id" = String, Path, description = "Resource identifier")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = ConfigDocument),
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
async fn get_prompts_id(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<ConfigDocument>> {
    let operation = { HostOperation::Config(ConfigOperation::GetPrompt { id: resource(id)? }) };
    run(&state, &caller, operation).await
}

#[utoipa::path(put, operation_id = "configuration_put_prompts_id", path = "/prompts/{id}", summary = "Update prompts", tag = "configuration",
    request_body = ConfigWrite,
    params(("id" = String, Path, description = "Resource identifier"), ("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = ConfigDocument),
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
async fn put_prompts_id(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiPath(id): ApiPath<String>,
    ApiJson(input): ApiJson<ConfigWrite>,
) -> ApiResult<Json<ConfigDocument>> {
    let operation = HostOperation::Config(ConfigOperation::PutPrompt {
        id: resource(id)?,
        input: input,
    });
    run(&state, &caller, operation).await
}
