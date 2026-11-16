//! Owner-managed assignments for the existing opt-in child model routing.
use axum::{extract::State, http::HeaderMap, Json};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::{
    actor::OwnerActor, host::run, input::resource, input::revision, input::ApiJson, input::ApiPath,
    input::IgnoredBody,
};
use crate::api::{
    errors::ApiResult, host::HostOperation, host::ModelOperation, host::ModelRoleOperation,
    models::ApiErrorBody, models::ModelPoolView, models::ModelRoleWrite, state::ApiState,
};
pub(super) fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(create))
        .routes(routes!(update))
        .routes(routes!(remove))
}
#[utoipa::path(post, operation_id = "model_roles_create", path = "/model-roles", summary = "Create model role", tag = "models",
 request_body = ModelRoleWrite,
 params(("x-albert-csrf" = String, Header, description = "Current browser CSRF value")), security(("browserCookie" = [])), responses((status = 200, description = "Updated pool", body = ModelPoolView),
(status = 400, description = "Invalid assignment", body = ApiErrorBody),
(status = 401, description = "Sign-in required", body = ApiErrorBody),
(status = 403, description = "Owner required", body = ApiErrorBody),
(status = 404, description = "Role unavailable", body = ApiErrorBody),
(status = 409, description = "Role already exists", body = ApiErrorBody),
(status = 412, description = "Revision changed", body = ApiErrorBody),
(status = 413, description = "Body too large", body = ApiErrorBody),
(status = 503, description = "Runtime unavailable", body = ApiErrorBody))
)]
async fn create(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiJson(input): ApiJson<ModelRoleWrite>,
) -> ApiResult<Json<ModelPoolView>> {
    run(
        &state,
        &caller,
        HostOperation::Model(ModelOperation::Role(ModelRoleOperation::Create(input))),
    )
    .await
}

#[utoipa::path(put, operation_id = "model_roles_update", path = "/model-roles/{id}", summary = "Update model role", tag = "models",
 request_body = ModelRoleWrite,
 params(("id" = String, Path, description = "Role name"),("x-albert-csrf" = String, Header, description = "Current browser CSRF value")), security(("browserCookie" = [])), responses((status = 200, description = "Updated pool", body = ModelPoolView),
(status = 400, description = "Invalid assignment", body = ApiErrorBody),
(status = 401, description = "Sign-in required", body = ApiErrorBody),
(status = 403, description = "Owner required", body = ApiErrorBody),
(status = 404, description = "Role unavailable", body = ApiErrorBody),
(status = 409, description = "Role already exists", body = ApiErrorBody),
(status = 412, description = "Revision changed", body = ApiErrorBody),
(status = 413, description = "Body too large", body = ApiErrorBody),
(status = 503, description = "Runtime unavailable", body = ApiErrorBody))
)]
async fn update(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiPath(id): ApiPath<String>,
    ApiJson(input): ApiJson<ModelRoleWrite>,
) -> ApiResult<Json<ModelPoolView>> {
    run(
        &state,
        &caller,
        HostOperation::Model(ModelOperation::Role(ModelRoleOperation::Update {
            id: resource(id)?,
            input,
        })),
    )
    .await
}

#[utoipa::path(delete, operation_id = "model_roles_remove", path = "/model-roles/{id}", summary = "Remove model role", tag = "models",

 params(("id" = String, Path, description = "Role name"),("If-Match" = String, Header, description = "Exact pool revision"),("x-albert-csrf" = String, Header, description = "Current browser CSRF value")), security(("browserCookie" = [])), responses((status = 200, description = "Updated pool", body = ModelPoolView),
(status = 400, description = "Invalid assignment", body = ApiErrorBody),
(status = 401, description = "Sign-in required", body = ApiErrorBody),
(status = 403, description = "Owner required", body = ApiErrorBody),
(status = 404, description = "Role unavailable", body = ApiErrorBody),
(status = 409, description = "Role already exists", body = ApiErrorBody),
(status = 412, description = "Revision changed", body = ApiErrorBody),
(status = 413, description = "Body too large", body = ApiErrorBody),
(status = 503, description = "Runtime unavailable", body = ApiErrorBody))
)]
async fn remove(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiPath(id): ApiPath<String>,
    headers: HeaderMap,
    _body: IgnoredBody,
) -> ApiResult<Json<ModelPoolView>> {
    run(
        &state,
        &caller,
        HostOperation::Model(ModelOperation::Role(ModelRoleOperation::Delete {
            id: resource(id)?,
            revision: revision(&headers)?,
        })),
    )
    .await
}
