//! Transport validation shared by typed domain handlers.
use axum::{
    body::Bytes, extract::FromRequest, extract::FromRequestParts, extract::Path, extract::Query,
    extract::Request, http::header::IF_MATCH, http::request::Parts, http::HeaderMap,
};
use serde::de::DeserializeOwned;
use serde_json::from_slice;

use crate::api::{errors::ApiError, errors::ApiResult, models::PageQuery, state::ApiState};

pub struct ApiJson<T>(pub T);
pub struct IgnoredBody;
impl FromRequest<ApiState> for IgnoredBody {
    type Rejection = ApiError;
    async fn from_request(request: Request, state: &ApiState) -> ApiResult<Self> {
        Bytes::from_request(request, state)
            .await
            .map(|_| Self)
            .map_err(|error| {
                ApiError::new(
                    error.status(),
                    "invalid_request",
                    "Request body exceeds its limit or could not be read.",
                )
            })
    }
}
impl<T: DeserializeOwned + Send> FromRequest<ApiState> for ApiJson<T> {
    type Rejection = ApiError;
    async fn from_request(request: Request, state: &ApiState) -> ApiResult<Self> {
        let bytes = Bytes::from_request(request, state).await.map_err(|error| {
            ApiError::new(
                error.status(),
                "invalid_request",
                "Request body exceeds its limit or could not be read.",
            )
        })?;
        from_slice(&bytes)
            .map(Self)
            .map_err(|_| ApiError::invalid("The request body does not match the expected object."))
    }
}

pub struct ApiPath<T>(pub T);
impl<T: DeserializeOwned + Send> FromRequestParts<ApiState> for ApiPath<T> {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &ApiState) -> ApiResult<Self> {
        Path::<T>::from_request_parts(parts, state)
            .await
            .map(|Path(value)| Self(value))
            .map_err(|_| ApiError::invalid("Invalid resource path."))
    }
}

pub struct ApiQuery<T>(pub T);
impl<T: DeserializeOwned + Send> FromRequestParts<ApiState> for ApiQuery<T> {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &ApiState) -> ApiResult<Self> {
        Query::<T>::from_request_parts(parts, state)
            .await
            .map(|Query(value)| Self(value))
            .map_err(|_| ApiError::invalid("Invalid query parameters."))
    }
}

pub fn resource(value: String) -> ApiResult<String> {
    if value.is_empty() || value.len() > 2048 || value.contains('\0') {
        Err(ApiError::invalid("Invalid resource ID."))
    } else {
        Ok(value)
    }
}
pub fn page(mut value: PageQuery) -> ApiResult<PageQuery> {
    let limit = value.limit.unwrap_or(10);
    if !(1..=100).contains(&limit) || value.order.as_deref().is_some_and(|order| order != "desc") {
        return Err(ApiError::invalid(
            "Pages require order=desc and limit between 1 and 100.",
        ));
    }
    value.limit = Some(limit);
    value.order = Some("desc".into());
    Ok(value)
}
pub fn revision(headers: &HeaderMap) -> ApiResult<Option<String>> {
    headers
        .get(IF_MATCH)
        .map(|value| {
            let value = value
                .to_str()
                .map_err(|_| ApiError::invalid("Invalid If-Match revision."))?;
            if value.starts_with("W/") || value.contains(',') || value == "*" {
                return Err(ApiError::invalid("Use one exact revision in If-Match."));
            }
            Ok(value.trim_matches('"').to_owned())
        })
        .transpose()
}
