//! subscription catalog HTTP resources.
use super::super::input::IgnoredBody;
use axum::{extract::State, Json};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::super::{actor::OwnerActor, host::run, input::ApiJson};
use crate::api::models::{
    ApiErrorBody, SubscriptionApplyRequest, SubscriptionCatalogView, SubscriptionConnectionView,
    SubscriptionSelection,
};
use crate::api::{
    errors::ApiResult, host::HostOperation, host::SubscriptionOperation, state::ApiState,
};

pub(super) fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(get_openai_subscription))
        .routes(routes!(post_openai_subscription_catalogs))
        .routes(routes!(post_openai_subscription_selections))
        .routes(routes!(post_openai_subscription_applications))
}

#[utoipa::path(get, operation_id = "subscription_get_openai_subscription", path = "/openai-subscription", summary = "Get openai subscription", tag = "subscription",
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = SubscriptionConnectionView),
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
async fn get_openai_subscription(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
) -> ApiResult<Json<SubscriptionConnectionView>> {
    let operation = { HostOperation::OpenAiSubscription(SubscriptionOperation::Connection) };
    run(&state, &caller, operation).await
}

#[utoipa::path(post, operation_id = "subscription_post_openai_subscription_catalogs", path = "/openai-subscription/catalogs", summary = "Create openai subscription catalogs", tag = "subscription",
    params(("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = SubscriptionCatalogView),
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
async fn post_openai_subscription_catalogs(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    _body: IgnoredBody,
) -> ApiResult<Json<SubscriptionCatalogView>> {
    let operation = { HostOperation::OpenAiSubscription(SubscriptionOperation::Catalog) };
    run(&state, &caller, operation).await
}

#[utoipa::path(post, operation_id = "subscription_post_openai_subscription_selections", path = "/openai-subscription/selections", summary = "Create openai subscription selections", tag = "subscription",
    request_body = SubscriptionSelection,
    params(("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = SubscriptionConnectionView),
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
async fn post_openai_subscription_selections(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiJson(input): ApiJson<SubscriptionSelection>,
) -> ApiResult<Json<SubscriptionConnectionView>> {
    let operation = { HostOperation::OpenAiSubscription(SubscriptionOperation::Select(input)) };
    run(&state, &caller, operation).await
}

#[utoipa::path(post, operation_id = "subscription_post_openai_subscription_applications", path = "/openai-subscription/applications", summary = "Create openai subscription applications", tag = "subscription",
    request_body = SubscriptionApplyRequest,
    params(("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = SubscriptionConnectionView),
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
async fn post_openai_subscription_applications(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiJson(input): ApiJson<SubscriptionApplyRequest>,
) -> ApiResult<Json<SubscriptionConnectionView>> {
    let operation = { HostOperation::OpenAiSubscription(SubscriptionOperation::Apply(input)) };
    run(&state, &caller, operation).await
}
