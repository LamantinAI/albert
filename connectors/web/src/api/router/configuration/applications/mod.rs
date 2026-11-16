//! configuration applications HTTP resources.
use axum::{extract::State, Json};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::super::{actor::OwnerActor, host::run, input::resource, input::ApiJson, input::ApiPath};
use crate::api::models::{ApiErrorBody, ApplyRequest, ApplyView, ConfigWrite, ValidationView};
use crate::api::{errors::ApiResult, host::ConfigOperation, host::HostOperation, state::ApiState};

pub(super) fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(post_configuration_validations))
        .routes(routes!(post_configuration_applications))
        .routes(routes!(get_configuration_applications_id))
}

#[utoipa::path(post, operation_id = "configuration_post_configuration_validations", path = "/configuration-validations", summary = "Create configuration validations", tag = "configuration",
    request_body = ConfigWrite,
    params(("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = ValidationView),
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
async fn post_configuration_validations(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiJson(input): ApiJson<ConfigWrite>,
) -> ApiResult<Json<ValidationView>> {
    let operation = { HostOperation::Config(ConfigOperation::Validate(input)) };
    run(&state, &caller, operation).await
}

#[utoipa::path(post, operation_id = "configuration_post_configuration_applications", path = "/configuration/applications", summary = "Create configuration applications", tag = "configuration",
    request_body = ApplyRequest,
    params(("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = ApplyView),
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
async fn post_configuration_applications(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiJson(input): ApiJson<ApplyRequest>,
) -> ApiResult<Json<ApplyView>> {
    let operation = { HostOperation::Config(ConfigOperation::Apply(input)) };
    run(&state, &caller, operation).await
}

#[utoipa::path(get, operation_id = "configuration_get_configuration_applications_id", path = "/configuration/applications/{id}", summary = "Get configuration applications", tag = "configuration",
    params(("id" = String, Path, description = "Resource identifier")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = ApplyView),
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
async fn get_configuration_applications_id(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<ApplyView>> {
    let operation = { HostOperation::Config(ConfigOperation::Application { id: resource(id)? }) };
    run(&state, &caller, operation).await
}
