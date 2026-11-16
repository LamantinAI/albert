//! profile HTTP resources.
use axum::{extract::State, Json};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::{actor::BrowserActor, host::run};
use crate::api::models::{ApiErrorBody, IdentityList, ProfileView};
use crate::api::{errors::ApiResult, host::HostOperation, host::ProfileOperation, state::ApiState};

pub(super) fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(get_account_profile))
        .routes(routes!(get_account_identities))
}

#[utoipa::path(get, operation_id = "profile_get_account_profile", path = "/account/profile", summary = "Get account profile", tag = "profile",
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = ProfileView),
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
async fn get_account_profile(
    State(state): State<ApiState>,
    BrowserActor(caller): BrowserActor,
) -> ApiResult<Json<ProfileView>> {
    let operation = HostOperation::Profile(ProfileOperation::Get);
    run(&state, &caller, operation).await
}

#[utoipa::path(get, operation_id = "profile_get_account_identities", path = "/account/identities", summary = "Get account identities", tag = "profile",
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = IdentityList),
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
async fn get_account_identities(
    State(state): State<ApiState>,
    BrowserActor(caller): BrowserActor,
) -> ApiResult<Json<IdentityList>> {
    let operation = HostOperation::Profile(ProfileOperation::Identities);
    run(&state, &caller, operation).await
}
