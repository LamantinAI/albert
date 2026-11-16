//! configuration documents HTTP resources.
use axum::{extract::State, Json};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::super::{actor::OwnerActor, host::run, input::resource, input::ApiJson, input::ApiPath};
use crate::api::models::{
    ApiErrorBody, ConfigDocument, ConfigDraftInput, ConfigDraftView, ConfigList, ConfigWrite,
};
use crate::api::{errors::ApiResult, host::ConfigOperation, host::HostOperation, state::ApiState};

pub(super) fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(get_configuration))
        .routes(routes!(get_configuration_id))
        .routes(routes!(post_configuration_id_drafts))
        .routes(routes!(put_configuration_id))
}

#[utoipa::path(get, operation_id = "configuration_get_configuration", path = "/configuration", summary = "Get configuration", tag = "configuration",
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = ConfigList),
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
async fn get_configuration(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
) -> ApiResult<Json<ConfigList>> {
    let operation = HostOperation::Config(ConfigOperation::List);
    run(&state, &caller, operation).await
}

#[utoipa::path(get, operation_id = "configuration_get_configuration_id", path = "/configuration/{id}", summary = "Get configuration", tag = "configuration",
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
async fn get_configuration_id(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<ConfigDocument>> {
    let operation = { HostOperation::Config(ConfigOperation::Get { id: resource(id)? }) };
    run(&state, &caller, operation).await
}

#[utoipa::path(post, operation_id = "configuration_post_configuration_id_drafts", path = "/configuration/{id}/drafts", summary = "Create configuration drafts", tag = "configuration",
    request_body = ConfigDraftInput,
    params(("id" = String, Path, description = "Resource identifier"), ("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = ConfigDraftView),
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
async fn post_configuration_id_drafts(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiPath(id): ApiPath<String>,
    ApiJson(input): ApiJson<ConfigDraftInput>,
) -> ApiResult<Json<ConfigDraftView>> {
    let operation = HostOperation::Config(ConfigOperation::Draft {
        id: resource(id)?,
        input: input,
    });
    run(&state, &caller, operation).await
}

#[utoipa::path(put, operation_id = "configuration_put_configuration_id", path = "/configuration/{id}", summary = "Update configuration", tag = "configuration",
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
async fn put_configuration_id(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiPath(id): ApiPath<String>,
    ApiJson(input): ApiJson<ConfigWrite>,
) -> ApiResult<Json<ConfigDocument>> {
    let operation = HostOperation::Config(ConfigOperation::Put {
        id: resource(id)?,
        input: input,
    });
    run(&state, &caller, operation).await
}
