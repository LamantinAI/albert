//! Explicit irreversible deletion of an owned permanent conversation.
use axum::{extract::State, http::HeaderMap, http::StatusCode};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::super::{
    actor::BrowserActor, host::run, input::resource, input::revision, input::ApiPath,
    input::IgnoredBody,
};
use crate::api::{
    errors::ApiError, errors::ApiResult, host::HostOperation, host::SessionOperation,
    models::ApiErrorBody, models::EmptyObject, state::ApiState,
};

pub(super) fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new().routes(routes!(delete_session))
}
#[utoipa::path(delete, operation_id = "sessions_delete_session", path = "/sessions/{id}", tag = "sessions",
    summary = "Delete an owned permanent conversation",
    description = "Erases the conversation and its attachments. A revision is required. Retrying an owned deletion resumes pending cleanup; 204 means cleanup completed.",
    params(("id" = String, Path, description = "Conversation identifier"),
        ("If-Match" = String, Header, description = "Exact conversation revision"),
        ("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 204, description = "Deleted and cleanup completed"),
        (status = 400, description = "Invalid revision or identifier", body = ApiErrorBody),
        (status = 401, description = "Sign-in required", body = ApiErrorBody),
        (status = 403, description = "Private conversation ownership required", body = ApiErrorBody),
        (status = 404, description = "Conversation not found", body = ApiErrorBody),
        (status = 409, description = "Conversation changed", body = ApiErrorBody),
        (status = 412, description = "Revision changed", body = ApiErrorBody),
        (status = 428, description = "If-Match required", body = ApiErrorBody),
        (status = 503, description = "Deletion cleanup pending; retry the same deletion", body = ApiErrorBody))
)]
async fn delete_session(
    State(state): State<ApiState>,
    BrowserActor(caller): BrowserActor,
    ApiPath(id): ApiPath<String>,
    headers: HeaderMap,
    _body: IgnoredBody,
) -> ApiResult<StatusCode> {
    let expected = revision(&headers)?.ok_or_else(|| {
        ApiError::new(
            StatusCode::PRECONDITION_REQUIRED,
            "revision_required",
            "An exact chat revision is required in If-Match.",
        )
    })?;
    let expected: i64 = expected
        .parse()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| ApiError::invalid("Invalid chat revision."))?;
    let _ = run::<EmptyObject>(
        &state,
        &caller,
        HostOperation::Session(SessionOperation::Delete {
            id: resource(id)?,
            revision: expected,
        }),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use axum::{
        body::to_bytes, body::Body, http::header::COOKIE, http::Method, http::Request,
        http::StatusCode,
    };
    use octo_history::SessionId;
    use std::sync::atomic::Ordering;
    use tower::ServiceExt;

    use crate::{
        api::models::CredentialUpdate, api::models::LoginRequest, api::testing::fixture,
        api::testing::ORIGIN,
    };

    #[tokio::test]
    async fn deletion_requires_authentication_csrf_and_one_positive_exact_revision() {
        let fixture = fixture().await;
        let login = fixture
            .auth
            .login(LoginRequest {
                username: "owner".into(),
                password: "owner".into(),
            })
            .await
            .unwrap();
        let principal = fixture.auth.authenticate(&login.token).await.unwrap();
        let (_, login) = fixture
            .auth
            .rotate_credentials(
                &principal,
                CredentialUpdate {
                    current_password: "owner".into(),
                    new_password: "rotated-fixture-password".into(),
                    new_username: None,
                },
            )
            .await
            .unwrap();
        let principal = fixture.auth.authenticate(&login.token).await.unwrap();
        let path = format!("/api/v1/sessions/{}", SessionId::new());
        for (authenticated, csrf, revision, expected) in [
            (false, false, Some("1"), StatusCode::UNAUTHORIZED),
            (true, false, Some("1"), StatusCode::FORBIDDEN),
            (true, true, None, StatusCode::PRECONDITION_REQUIRED),
            (true, true, Some("W/\"1\""), StatusCode::BAD_REQUEST),
            (true, true, Some("*"), StatusCode::BAD_REQUEST),
            (true, true, Some("1,2"), StatusCode::BAD_REQUEST),
            (true, true, Some("0"), StatusCode::BAD_REQUEST),
            (true, true, Some("-1"), StatusCode::BAD_REQUEST),
            (true, true, Some("\"1\""), StatusCode::NO_CONTENT),
        ] {
            let mut request = Request::builder()
                .method(Method::DELETE)
                .uri(&path)
                .header("host", "localhost:9877")
                .header("origin", ORIGIN);
            if authenticated {
                request = request.header(COOKIE, format!("albert_auth={}", login.token));
            }
            if csrf {
                request = request.header("x-albert-csrf", principal.csrf());
            }
            if let Some(revision) = revision {
                request = request.header("If-Match", revision);
            }
            let response = fixture
                .router
                .clone()
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), expected);
            if expected == StatusCode::NO_CONTENT {
                assert!(to_bytes(response.into_body(), 1024)
                    .await
                    .unwrap()
                    .is_empty());
            }
        }
        assert_eq!(fixture.host.calls.load(Ordering::SeqCst), 1);
    }
}
