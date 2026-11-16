//! configuration secrets HTTP resources.
use super::super::input::IgnoredBody;
use axum::{extract::State, Json};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::super::{actor::OwnerActor, host::run, input::resource, input::ApiJson, input::ApiPath};
use crate::api::models::{ApiErrorBody, EmptyObject, SecretList, SecretWrite};
use crate::api::{errors::ApiResult, host::ConfigOperation, host::HostOperation, state::ApiState};

pub(super) fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(get_secrets))
        .routes(routes!(put_secrets_id))
        .routes(routes!(delete_secrets_id))
}

#[utoipa::path(get, operation_id = "configuration_get_secrets", path = "/secrets", summary = "Get secrets", tag = "configuration",
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = SecretList),
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
async fn get_secrets(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
) -> ApiResult<Json<SecretList>> {
    let operation = HostOperation::Config(ConfigOperation::Secrets);
    run(&state, &caller, operation).await
}

#[utoipa::path(put, operation_id = "configuration_put_secrets_id", path = "/secrets/{id}", summary = "Update secrets", tag = "configuration",
    request_body = SecretWrite,
    params(("id" = String, Path, description = "Resource identifier"), ("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = EmptyObject),
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
async fn put_secrets_id(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiPath(id): ApiPath<String>,
    ApiJson(input): ApiJson<SecretWrite>,
) -> ApiResult<Json<EmptyObject>> {
    let operation = HostOperation::Config(ConfigOperation::PutSecret {
        id: resource(id)?,
        input: input,
    });
    run(&state, &caller, operation).await
}

#[utoipa::path(delete, operation_id = "configuration_delete_secrets_id", path = "/secrets/{id}", summary = "Remove secrets", tag = "configuration",
    params(("id" = String, Path, description = "Resource identifier"), ("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = EmptyObject),
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
async fn delete_secrets_id(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiPath(id): ApiPath<String>,
    _body: IgnoredBody,
) -> ApiResult<Json<EmptyObject>> {
    let operation = { HostOperation::Config(ConfigOperation::DeleteSecret { id: resource(id)? }) };
    run(&state, &caller, operation).await
}
