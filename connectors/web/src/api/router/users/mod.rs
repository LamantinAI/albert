//! Account security and user administration.
use axum::{extract::State, http::HeaderMap, response::IntoResponse, response::Response, Json};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::{
    actor::OwnerActor, actor::RotationActor, input::resource, input::ApiJson, input::ApiPath,
    renewal::login_cookies,
};
use crate::api::{
    errors::ApiResult, extractors::secure_request, models::AccountView, models::ApiErrorBody,
    models::CredentialUpdate, models::UserCreate, models::UserList, models::UserUpdate,
    state::ApiState,
};

pub(super) fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(rotate))
        .routes(routes!(list))
        .routes(routes!(create))
        .routes(routes!(update))
}
#[utoipa::path(put, operation_id = "account_rotate", path = "/account/credentials", summary = "Update account credentials", tag = "account", request_body = CredentialUpdate, security(("browserCookie" = [])), params(("x-albert-csrf" = String, Header)),
    responses((status = 200, body = AccountView), (status = 400, body = ApiErrorBody), (status = 401, body = ApiErrorBody), (status = 403, body = ApiErrorBody), (status = 409, body = ApiErrorBody), (status = 429, body = ApiErrorBody), (status = 500, body = ApiErrorBody)))]
async fn rotate(
    State(state): State<ApiState>,
    RotationActor(caller): RotationActor,
    headers: HeaderMap,
    ApiJson(input): ApiJson<CredentialUpdate>,
) -> ApiResult<Response> {
    let (account, login) = state.auth.rotate_credentials(&caller, input).await?;
    let mut response = Json(account).into_response();
    login_cookies(
        &state,
        &mut response,
        &login,
        secure_request(&state, &headers)?,
    )?;
    Ok(response)
}
#[utoipa::path(get, operation_id = "users_list", path = "/users", summary = "Get users", tag = "users", security(("browserCookie" = [])),
    responses((status = 200, body = UserList), (status = 401, body = ApiErrorBody), (status = 403, body = ApiErrorBody), (status = 500, body = ApiErrorBody)))]
async fn list(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
) -> ApiResult<Json<UserList>> {
    Ok(Json(UserList {
        items: state.auth.list_users(&caller.owner_permit()?).await?,
    }))
}
#[utoipa::path(post, operation_id = "users_create", path = "/users", summary = "Create users", tag = "users", request_body = UserCreate, security(("browserCookie" = [])), params(("x-albert-csrf" = String, Header)),
    responses((status = 200, body = AccountView), (status = 400, body = ApiErrorBody), (status = 401, body = ApiErrorBody), (status = 403, body = ApiErrorBody), (status = 409, body = ApiErrorBody), (status = 500, body = ApiErrorBody)))]
async fn create(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiJson(input): ApiJson<UserCreate>,
) -> ApiResult<Json<AccountView>> {
    Ok(Json(
        state
            .auth
            .create_user(&caller.owner_permit()?, input)
            .await?,
    ))
}
#[utoipa::path(patch, operation_id = "users_update", path = "/users/{id}", summary = "Update users", tag = "users", request_body = UserUpdate, security(("browserCookie" = [])), params(("id" = String, Path), ("x-albert-csrf" = String, Header)),
    responses((status = 200, body = AccountView), (status = 400, body = ApiErrorBody), (status = 401, body = ApiErrorBody), (status = 403, body = ApiErrorBody), (status = 409, body = ApiErrorBody), (status = 500, body = ApiErrorBody)))]
async fn update(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiPath(id): ApiPath<String>,
    ApiJson(input): ApiJson<UserUpdate>,
) -> ApiResult<Json<AccountView>> {
    Ok(Json(
        state
            .auth
            .update_user(&caller.owner_permit()?, &resource(id)?, input)
            .await?,
    ))
}
