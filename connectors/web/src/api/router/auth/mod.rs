//! Browser login, renewal and credential revocation.
use axum::{extract::State, http::HeaderMap, response::IntoResponse, response::Response, Json};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::{
    actor::RotationActor, browser_context::set_cookie, browser_context::set_named_cookie,
    input::resource, input::ApiJson, input::ApiPath, input::IgnoredBody, renewal::login_cookies,
    renewal::refresh as renew_cookie, renewal::REFRESH_COOKIE,
};
use crate::api::{
    errors::ApiResult, extractors::login_origin, extractors::secure_request, models::ApiErrorBody,
    models::CredentialView, models::EmptyObject, models::LoginRequest, models::RefreshView,
    state::ApiState,
};

pub(super) fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(login))
        .routes(routes!(renew))
        .routes(routes!(logout))
}

#[utoipa::path(post, operation_id = "authentication_login", path = "/auth/credentials", summary = "Create auth credentials", tag = "authentication", request_body = LoginRequest,
    responses((status = 200, body = CredentialView), (status = 400, body = ApiErrorBody), (status = 401, body = ApiErrorBody), (status = 403, body = ApiErrorBody), (status = 413, body = ApiErrorBody), (status = 429, body = ApiErrorBody), (status = 500, body = ApiErrorBody)))]
async fn login(
    State(state): State<ApiState>,
    headers: HeaderMap,
    ApiJson(input): ApiJson<LoginRequest>,
) -> ApiResult<Response> {
    login_origin(&state, &headers, &input.username, &input.password)?;
    let login = state.auth.login(input).await?;
    let mut response = Json(login.credential.clone()).into_response();
    login_cookies(
        &state,
        &mut response,
        &login,
        secure_request(&state, &headers)?,
    )?;
    Ok(response)
}

#[utoipa::path(post, operation_id = "authentication_renew", path = "/auth/refresh", summary = "Create auth refresh", tag = "authentication", security(("refreshCookie" = [])),
    params(("x-albert-refresh" = String, Header, description = "Must be 1")),
    responses((status = 200, body = RefreshView), (status = 401, body = ApiErrorBody), (status = 403, body = ApiErrorBody), (status = 413, body = ApiErrorBody), (status = 500, body = ApiErrorBody)))]
async fn renew(
    State(state): State<ApiState>,
    headers: HeaderMap,
    _body: IgnoredBody,
) -> ApiResult<Response> {
    renew_cookie(&state, &headers).await
}

#[utoipa::path(delete, operation_id = "authentication_logout", path = "/auth/credentials/{id}", summary = "Remove auth credentials", tag = "authentication", security(("browserCookie" = [])),
    params(("id" = String, Path), ("x-albert-csrf" = String, Header)),
    responses((status = 200, body = EmptyObject), (status = 401, body = ApiErrorBody), (status = 403, body = ApiErrorBody), (status = 500, body = ApiErrorBody)))]
async fn logout(
    State(state): State<ApiState>,
    RotationActor(caller): RotationActor,
    ApiPath(id): ApiPath<String>,
    headers: HeaderMap,
    _body: IgnoredBody,
) -> ApiResult<Response> {
    let id = resource(id)?;
    state.auth.revoke_credential(&caller, &id).await?;
    let mut response = Json(EmptyObject {}).into_response();
    if id == caller.credential_id() {
        let secure = secure_request(&state, &headers)?;
        set_cookie(&state, &mut response, "", 0, secure)?;
        set_named_cookie(&state, &mut response, REFRESH_COOKIE, "", 0, secure)?;
    }
    Ok(response)
}
