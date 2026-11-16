//! telegram HTTP resources.
use axum::{extract::State, Json};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::{
    actor::BrowserActor, actor::OwnerActor, host::run, input::resource, input::ApiJson,
    input::ApiPath, input::IgnoredBody,
};
use crate::api::models::{
    ApiErrorBody, EmptyObject, TelegramConnectionList, TelegramConnectionView,
    TelegramCredentialWrite, TelegramLoginTokenView, TelegramTokenRequest,
};
use crate::api::{
    errors::ApiResult, host::HostOperation, host::TelegramOperation, state::ApiState,
};

pub(super) fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(get_account_telegram_connections))
        .routes(routes!(get_account_connectors_id_telegram))
        .routes(routes!(put_connectors_id_telegram_credential))
        .routes(routes!(post_account_connectors_id_telegram_login_tokens))
        .routes(routes!(
            delete_account_connectors_id_telegram_login_tokens_current
        ))
        .routes(routes!(delete_account_connectors_id_telegram_binding))
}

#[utoipa::path(get, operation_id = "telegram_get_account_telegram_connections", path = "/account/telegram/connections", summary = "Get account telegram connections", tag = "telegram",
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = TelegramConnectionList),
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
async fn get_account_telegram_connections(
    State(state): State<ApiState>,
    BrowserActor(caller): BrowserActor,
) -> ApiResult<Json<TelegramConnectionList>> {
    let operation = { HostOperation::Telegram(TelegramOperation::List) };
    run(&state, &caller, operation).await
}

#[utoipa::path(get, operation_id = "telegram_get_account_connectors_id_telegram", path = "/account/connectors/{id}/telegram", summary = "Get account connectors telegram", tag = "telegram",
    params(("id" = String, Path, description = "Resource identifier")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = TelegramConnectionView),
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
async fn get_account_connectors_id_telegram(
    State(state): State<ApiState>,
    BrowserActor(caller): BrowserActor,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<TelegramConnectionView>> {
    let operation = { HostOperation::Telegram(TelegramOperation::Status { id: resource(id)? }) };
    run(&state, &caller, operation).await
}

#[utoipa::path(put, operation_id = "telegram_put_connectors_id_telegram_credential", path = "/connectors/{id}/telegram/credential", summary = "Update connectors telegram credential", tag = "telegram",
    request_body = TelegramCredentialWrite,
    params(("id" = String, Path, description = "Resource identifier"), ("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = TelegramConnectionView),
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
async fn put_connectors_id_telegram_credential(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiPath(id): ApiPath<String>,
    ApiJson(input): ApiJson<TelegramCredentialWrite>,
) -> ApiResult<Json<TelegramConnectionView>> {
    let operation = {
        HostOperation::Telegram(TelegramOperation::Credential {
            id: resource(id)?,
            input: input,
        })
    };
    run(&state, &caller, operation).await
}

#[utoipa::path(post, operation_id = "telegram_post_account_connectors_id_telegram_login_tokens", path = "/account/connectors/{id}/telegram/login-tokens", summary = "Create account connectors telegram login tokens", tag = "telegram",
    request_body = TelegramTokenRequest,
    params(("id" = String, Path, description = "Resource identifier"), ("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = TelegramLoginTokenView),
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
async fn post_account_connectors_id_telegram_login_tokens(
    State(state): State<ApiState>,
    BrowserActor(caller): BrowserActor,
    ApiPath(id): ApiPath<String>,
    ApiJson(input): ApiJson<TelegramTokenRequest>,
) -> ApiResult<Json<TelegramLoginTokenView>> {
    let operation = {
        let _: TelegramTokenRequest = input;
        HostOperation::Telegram(TelegramOperation::Issue { id: resource(id)? })
    };
    run(&state, &caller, operation).await
}

#[utoipa::path(delete, operation_id = "telegram_delete_account_connectors_id_telegram_login_tokens_current", path = "/account/connectors/{id}/telegram/login-tokens/current", summary = "Remove account connectors telegram login tokens current", tag = "telegram",
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
async fn delete_account_connectors_id_telegram_login_tokens_current(
    State(state): State<ApiState>,
    BrowserActor(caller): BrowserActor,
    ApiPath(id): ApiPath<String>,
    _body: IgnoredBody,
) -> ApiResult<Json<EmptyObject>> {
    let operation =
        { HostOperation::Telegram(TelegramOperation::RevokeToken { id: resource(id)? }) };
    run(&state, &caller, operation).await
}

#[utoipa::path(delete, operation_id = "telegram_delete_account_connectors_id_telegram_binding", path = "/account/connectors/{id}/telegram/binding", summary = "Remove account connectors telegram binding", tag = "telegram",
    params(("id" = String, Path, description = "Resource identifier"), ("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = TelegramConnectionView),
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
async fn delete_account_connectors_id_telegram_binding(
    State(state): State<ApiState>,
    BrowserActor(caller): BrowserActor,
    ApiPath(id): ApiPath<String>,
    _body: IgnoredBody,
) -> ApiResult<Json<TelegramConnectionView>> {
    let operation =
        { HostOperation::Telegram(TelegramOperation::Disconnect { id: resource(id)? }) };
    run(&state, &caller, operation).await
}
