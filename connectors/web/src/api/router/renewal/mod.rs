use axum::{http::HeaderMap, response::IntoResponse, response::Response, Json};
use chrono::Utc;

use super::browser_context::{set_cookie, set_named_cookie};
use crate::{
    api::errors::ApiError, api::errors::ApiResult, api::extractors::named_cookie,
    api::extractors::same_origin, api::extractors::secure_request, api::models::RefreshView,
    api::state::ApiState, service::auth::LoginResult,
};

pub(super) const REFRESH_COOKIE: &str = "albert_refresh";

pub(super) fn login_cookies(
    state: &ApiState,
    response: &mut Response,
    login: &LoginResult,
    secure: bool,
) -> ApiResult<()> {
    let now = Utc::now().timestamp();
    set_cookie(
        state,
        response,
        &login.token,
        login.credential.expires_at.saturating_sub(now).max(0) as u64,
        secure,
    )?;
    if let Some(refresh) = &login.refresh {
        set_named_cookie(
            state,
            response,
            REFRESH_COOKIE,
            &refresh.token,
            refresh.expires_at.saturating_sub(now).max(0) as u64,
            secure,
        )?;
    }
    Ok(())
}

pub(super) async fn refresh(state: &ApiState, headers: &HeaderMap) -> ApiResult<Response> {
    same_origin(state, headers)?;
    let mut values = headers.get_all("x-albert-refresh").iter();
    if values.next().is_none_or(|value| value != "1") || values.next().is_some() {
        return Err(ApiError::forbidden());
    }
    let token = named_cookie(headers, REFRESH_COOKIE).ok_or_else(ApiError::unauthorized)?;
    let login = state.auth.refresh(&token).await?;
    let mut response = Json(RefreshView {
        id: login.credential.id.clone(),
        expires_at: login.credential.expires_at,
        csrf: login.csrf.clone(),
    })
    .into_response();
    login_cookies(
        state,
        &mut response,
        &login,
        secure_request(state, headers)?,
    )?;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use axum::{
        body::to_bytes, body::Body, http::header::SET_COOKIE, http::HeaderMap, http::Method,
        http::Request, http::StatusCode,
    };
    use serde_json::{from_slice, json, Value};
    use tower::ServiceExt;

    use crate::{
        api::testing::fixture_with_origin, api::testing::request, service::auth::AuthError,
    };

    fn cookies(headers: &HeaderMap) -> Vec<String> {
        headers
            .get_all(SET_COOKIE)
            .iter()
            .map(|header| {
                header
                    .to_str()
                    .unwrap()
                    .split(';')
                    .next()
                    .unwrap()
                    .to_owned()
            })
            .collect()
    }

    #[tokio::test]
    async fn refresh_cookie_is_http_only_scoped_revocable_and_never_returned_as_json() {
        let fixture = fixture_with_origin("https://example.org").await;
        let (status, headers, body) = request(
            &fixture.router,
            Method::POST,
            "/api/v1/auth/credentials",
            None,
            None,
            Some("https://example.org"),
            json!({"username":"owner","password":"owner"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let login = cookies(&headers);
        assert_eq!(login.len(), 2);
        for value in headers.get_all(SET_COOKIE) {
            let value = value.to_str().unwrap();
            assert!(
                value.contains("HttpOnly")
                    && value.contains("SameSite=Strict")
                    && value.contains("; Secure")
                    && value.contains("Path=/")
            );
        }
        let refresh = login
            .iter()
            .find(|cookie| cookie.starts_with("albert_refresh="))
            .unwrap();
        let secret = refresh.strip_prefix("albert_refresh=").unwrap();
        assert!(!body.to_string().contains(secret));
        for (origin, marker, cookie, expected) in [
            (
                "https://example.org",
                None,
                refresh.clone(),
                StatusCode::FORBIDDEN,
            ),
            (
                "https://other.example",
                Some("1"),
                refresh.clone(),
                StatusCode::FORBIDDEN,
            ),
            (
                "https://example.org",
                Some("1"),
                format!("{refresh}; {refresh}"),
                StatusCode::UNAUTHORIZED,
            ),
        ] {
            let mut request = Request::post("/api/v1/auth/refresh")
                .header("origin", origin)
                .header("cookie", cookie);
            if let Some(marker) = marker {
                request = request.header("x-albert-refresh", marker);
            }
            let response = fixture
                .router
                .clone()
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), expected);
            assert!(response.headers().get(SET_COOKIE).is_none());
        }
        let response = fixture
            .router
            .clone()
            .oneshot(
                Request::post("/api/v1/auth/refresh")
                    .header("origin", "https://example.org")
                    .header("cookie", refresh)
                    .header("x-albert-refresh", "1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let renewed_cookie = cookies(response.headers());
        assert_eq!(renewed_cookie.len(), 1);
        let renewed: Value =
            from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        assert!(!renewed.to_string().contains(secret));
        assert_eq!(renewed["id"], body["id"]);
        let (status, expired, _) = request(
            &fixture.router,
            Method::DELETE,
            &format!("/api/v1/auth/credentials/{}", body["id"].as_str().unwrap()),
            Some(&renewed_cookie[0]),
            renewed["csrf"].as_str(),
            Some("https://example.org"),
            json!({}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(cookies(&expired).len(), 2);
        assert!(expired
            .get_all(SET_COOKIE)
            .iter()
            .all(|value| value.to_str().unwrap().contains("Max-Age=0")));
        assert!(matches!(
            fixture.auth.refresh(secret).await,
            Err(AuthError::Unauthorized)
        ));
    }
}
