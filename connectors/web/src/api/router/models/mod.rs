//! models HTTP resources.
use axum::{extract::State, http::HeaderMap, Json};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::{
    actor::OwnerActor, host::run, input::resource, input::revision, input::ApiJson, input::ApiPath,
    input::IgnoredBody,
};
use crate::api::models::{ApiErrorBody, ModelOrder, ModelPoolView, ModelPreference, ModelWrite};
use crate::api::{errors::ApiResult, host::HostOperation, host::ModelOperation, state::ApiState};

pub(super) fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(get_model_pool))
        .routes(routes!(post_models))
        .routes(routes!(put_models_id))
        .routes(routes!(delete_models_id))
        .routes(routes!(put_model_pool_order))
        .routes(routes!(put_model_pool_preference))
}

#[utoipa::path(get, operation_id = "models_get_model_pool", path = "/model-pool", summary = "Get model pool", tag = "models",
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = ModelPoolView),
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
async fn get_model_pool(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
) -> ApiResult<Json<ModelPoolView>> {
    let operation = HostOperation::Model(ModelOperation::List);
    run(&state, &caller, operation).await
}

#[utoipa::path(post, operation_id = "models_post_models", path = "/models", summary = "Create models", tag = "models",
    request_body = ModelWrite,
    params(("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = ModelPoolView),
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
async fn post_models(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiJson(input): ApiJson<ModelWrite>,
) -> ApiResult<Json<ModelPoolView>> {
    let operation = HostOperation::Model(ModelOperation::Create(input));
    run(&state, &caller, operation).await
}

#[utoipa::path(put, operation_id = "models_put_models_id", path = "/models/{id}", summary = "Update models", tag = "models",
    request_body = ModelWrite,
    params(("id" = String, Path, description = "Resource identifier"), ("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = ModelPoolView),
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
async fn put_models_id(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiPath(id): ApiPath<String>,
    ApiJson(input): ApiJson<ModelWrite>,
) -> ApiResult<Json<ModelPoolView>> {
    let operation = HostOperation::Model(ModelOperation::Update {
        id: resource(id)?,
        input: input,
    });
    run(&state, &caller, operation).await
}

#[utoipa::path(delete, operation_id = "models_delete_models_id", path = "/models/{id}", summary = "Remove models", tag = "models",
    params(("id" = String, Path, description = "Resource identifier"), ("If-Match" = Option<String>, Header, description = "Exact configuration revision"), ("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = ModelPoolView),
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
async fn delete_models_id(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiPath(id): ApiPath<String>,
    headers: HeaderMap,
    _body: IgnoredBody,
) -> ApiResult<Json<ModelPoolView>> {
    let operation = HostOperation::Model(ModelOperation::Delete {
        id: resource(id)?,
        revision: revision(&headers)?,
    });
    run(&state, &caller, operation).await
}

#[utoipa::path(put, operation_id = "models_put_model_pool_order", path = "/model-pool/order", summary = "Update model pool order", tag = "models",
    request_body = ModelOrder,
    params(("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = ModelPoolView),
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
async fn put_model_pool_order(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiJson(input): ApiJson<ModelOrder>,
) -> ApiResult<Json<ModelPoolView>> {
    let operation = HostOperation::Model(ModelOperation::Order(input));
    run(&state, &caller, operation).await
}

#[utoipa::path(put, operation_id = "models_put_model_pool_preference", path = "/model-pool/preference", summary = "Update model pool preference", tag = "models",
    request_body = ModelPreference,
    params(("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = ModelPoolView),
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
async fn put_model_pool_preference(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiJson(input): ApiJson<ModelPreference>,
) -> ApiResult<Json<ModelPoolView>> {
    let operation = { HostOperation::Model(ModelOperation::Preference(input)) };
    run(&state, &caller, operation).await
}
