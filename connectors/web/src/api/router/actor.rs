//! Browser authentication and mutation guards precede domain request parsing.
use axum::{extract::FromRequestParts, http::request::Parts, http::Method, http::StatusCode};

use crate::api::{
    errors::ApiError, errors::ApiResult, extractors::csrf, extractors::principal,
    principal::Principal, state::ApiState,
};

pub struct BrowserActor(pub Principal);
pub struct OwnerActor(pub Principal);
pub struct RotationActor(pub Principal);

async fn authenticated(
    parts: &Parts,
    state: &ApiState,
    allow_rotation: bool,
) -> ApiResult<Principal> {
    let caller = principal(state, &parts.headers).await?;
    if !matches!(parts.method, Method::GET | Method::HEAD) {
        csrf(state, &parts.headers, &caller)?;
    }
    if caller.must_rotate() && !allow_rotation {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "credential_rotation_required",
            "Change the default credentials before continuing.",
        ));
    }
    Ok(caller)
}
impl FromRequestParts<ApiState> for BrowserActor {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &ApiState) -> ApiResult<Self> {
        authenticated(parts, state, false).await.map(Self)
    }
}
impl FromRequestParts<ApiState> for OwnerActor {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &ApiState) -> ApiResult<Self> {
        let caller = authenticated(parts, state, false).await?;
        caller.owner_permit()?;
        Ok(Self(caller))
    }
}
impl FromRequestParts<ApiState> for RotationActor {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &ApiState) -> ApiResult<Self> {
        authenticated(parts, state, true).await.map(Self)
    }
}
