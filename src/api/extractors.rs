use axum::http::{
    header::{COOKIE, HOST, ORIGIN},
    HeaderMap,
};
use url::Url;

use super::{
    errors::{ApiError, ApiResult},
    principal::Principal,
    state::ApiState,
};

pub const COOKIE_NAME: &str = "albert_auth";
pub const CSRF_HEADER: &str = "x-albert-csrf";

pub fn cookie_token(headers: &HeaderMap) -> Option<String> {
    let mut matches = headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|v| v.trim().split_once('='))
        .filter(|(key, _)| *key == COOKIE_NAME);
    let token = matches.next()?.1;
    if matches.next().is_some() {
        return None;
    }
    Some(token.into())
}
pub async fn principal(state: &ApiState, headers: &HeaderMap) -> ApiResult<Principal> {
    state
        .auth
        .authenticate(&cookie_token(headers).ok_or_else(ApiError::unauthorized)?)
        .await
        .map_err(Into::into)
}
pub fn same_origin(state: &ApiState, headers: &HeaderMap) -> ApiResult<()> {
    let presented = headers
        .get(ORIGIN)
        .and_then(|h| h.to_str().ok())
        .ok_or_else(ApiError::forbidden)?;
    let origin = Url::parse(presented).map_err(|_| ApiError::forbidden())?;
    if !matches!(origin.scheme(), "http" | "https")
        || origin.origin().ascii_serialization() != presented
    {
        return Err(ApiError::forbidden());
    }
    let configured = &state.auth.settings().public_origin;
    let expected = if configured.is_empty() {
        let host = headers
            .get(HOST)
            .and_then(|h| h.to_str().ok())
            .ok_or_else(ApiError::forbidden)?;
        Url::parse(&format!("{}://{host}", origin.scheme()))
            .map_err(|_| ApiError::forbidden())?
            .origin()
            .ascii_serialization()
    } else {
        configured.clone()
    };
    if presented != expected {
        return Err(ApiError::forbidden());
    }
    Ok(())
}
pub fn csrf(state: &ApiState, headers: &HeaderMap, principal: &Principal) -> ApiResult<()> {
    same_origin(state, headers)?;
    let presented = headers
        .get(CSRF_HEADER)
        .and_then(|h| h.to_str().ok())
        .ok_or_else(ApiError::forbidden)?;
    let expected = principal.csrf();
    if presented.len() != expected.len()
        || presented
            .as_bytes()
            .iter()
            .zip(expected.as_bytes())
            .fold(0u8, |a, (x, y)| a | (x ^ y))
            != 0
    {
        return Err(ApiError::forbidden());
    }
    Ok(())
}
