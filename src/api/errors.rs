use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

use super::models::{ApiErrorBody, ApiErrorDetail};
use crate::service::auth::AuthError;

pub type ApiResult<T> = Result<T, ApiError>;
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}
impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_request", message)
    }
    pub fn forbidden() -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "This action is not permitted.",
        )
    }
    pub fn unauthorized() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "Sign in to continue.",
        )
    }
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, "conflict", message)
    }
    pub fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", "Resource not found.")
    }
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, "unavailable", message)
    }
    pub fn internal() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "The service could not complete this request.",
        )
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ApiErrorBody {
                error: ApiErrorDetail {
                    code: self.code.into(),
                    message: self.message,
                },
            }),
        )
            .into_response()
    }
}
impl From<AuthError> for ApiError {
    fn from(error: AuthError) -> Self {
        match error {
            AuthError::InvalidCredentials => Self::new(
                StatusCode::UNAUTHORIZED,
                "invalid_credentials",
                "Username or password is incorrect.",
            ),
            AuthError::Unauthorized => Self::unauthorized(),
            AuthError::Forbidden => Self::forbidden(),
            AuthError::Busy => Self::new(
                StatusCode::TOO_MANY_REQUESTS,
                "authentication_busy",
                "Try again shortly.",
            ),
            AuthError::InvalidInput(message) => Self::invalid(message),
            AuthError::Conflict => Self::conflict("Account changed. Reload and retry."),
            AuthError::Database(error)
                if error
                    .as_database_error()
                    .is_some_and(|e| e.is_unique_violation()) =>
            {
                Self::conflict("This username is already in use.")
            }
            _ => Self::internal(),
        }
    }
}
