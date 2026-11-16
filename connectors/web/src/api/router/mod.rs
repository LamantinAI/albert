//! Typed resource router composition; handlers own their HTTP and OpenAPI contracts.
mod actor;
mod approvals;
mod attachments;
mod auth;
mod authorization;
mod browser_context;
mod configuration;
mod connections;
mod credentials;
mod host;
mod input;
mod mcp;
mod memory;
mod model_roles;
mod models;
mod operator;
mod profile;
mod renewal;
mod sessions;
mod skills;
mod stream;
mod subscription;
mod telegram;
mod users;

use axum::{
    extract::DefaultBodyLimit, extract::Request, extract::State, http::header::CACHE_CONTROL,
    http::HeaderMap, middleware::from_fn, middleware::Next, response::Response, routing::get, Json,
    Router,
};
use utoipa_axum::{router::OpenApiRouter, routes};

use self::browser_context::context;
use crate::api::{
    docs::schema, errors::ApiResult, models::ApiErrorBody, models::ClientContext, state::ApiState,
};

pub(super) fn domain_routes(media_limit: usize) -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(browser_context))
        .merge(operator::router())
        .merge(auth::router())
        .merge(users::router())
        .merge(credentials::router())
        .merge(connections::router())
        .merge(telegram::router())
        .merge(subscription::router())
        .merge(profile::router())
        .merge(sessions::router())
        .merge(models::router())
        .merge(mcp::router())
        .merge(memory::router())
        .merge(model_roles::router())
        .merge(skills::router())
        .merge(configuration::router())
        .merge(approvals::router())
        .merge(attachments::router(media_limit))
}

#[utoipa::path(get, operation_id = "account_browser_context", path = "/context", summary = "Get context", tag = "account", responses((status = 200, body = ClientContext), (status = 500, body = ApiErrorBody)))]
async fn browser_context(State(state): State<ApiState>, headers: HeaderMap) -> ApiResult<Response> {
    context(&state, &headers).await
}

#[cfg(test)]
pub fn router(state: ApiState) -> Router {
    router_with_body_limit(state, 4 * 1024 * 1024)
}
pub fn router_with_body_limit(state: ApiState, max_body_bytes: usize) -> Router {
    router_with_media_limit(state, max_body_bytes, max_body_bytes)
}
pub fn router_with_media_limit(
    state: ApiState,
    max_body_bytes: usize,
    media_bytes: usize,
) -> Router {
    let (routes, _) = domain_routes(media_bytes).split_for_parts();
    Router::new()
        .nest("/api/v1", routes)
        .route("/api/v1/openapi.json", get(|| async { Json(schema()) }))
        .route("/openapi.json", get(|| async { Json(schema()) }))
        .layer(DefaultBodyLimit::max(max_body_bytes))
        .layer(from_fn(response_headers))
        .with_state(state)
}
async fn response_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(CACHE_CONTROL, "no-store".parse().expect("static header"));
    response.headers_mut().insert(
        "x-content-type-options",
        "nosniff".parse().expect("static header"),
    );
    response
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use axum::{body::to_bytes, body::Body, http::Method, http::Request, http::StatusCode};
    use serde_json::{from_slice, json, Value};
    use tower::ServiceExt;

    use crate::{
        api::docs::schema, api::testing::cookie, api::testing::fixture, api::testing::request,
        api::testing::ORIGIN,
    };

    #[tokio::test]
    async fn configured_body_limit_is_enforced_before_dispatch() {
        use super::router_with_body_limit;
        use crate::api::state::ApiState;

        let fixture = fixture().await;
        let router = router_with_body_limit(ApiState::new(fixture.auth, fixture.host), 32);
        let response = router
            .oneshot(
                Request::post("/api/v1/auth/credentials")
                    .header("content-type", "application/json")
                    .header("origin", ORIGIN)
                    .body(Body::from("x".repeat(33)))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn default_owner_login_rotation_and_profile_work_without_model_or_telegram() {
        let fixture = fixture().await;
        let (status, _, signed_out) = request(
            &fixture.router,
            Method::GET,
            "/api/v1/context",
            None,
            None,
            None,
            json!({}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(signed_out["account"].is_null());
        assert_eq!(signed_out["setup"]["model_ready"], false);
        let (status, headers, credential) = request(
            &fixture.router,
            Method::POST,
            "/api/v1/auth/credentials",
            None,
            None,
            Some(ORIGIN),
            json!({"username":"owner","password":"owner"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(credential.get("token").is_none());
        let owner_cookie = cookie(&headers);
        let header = headers["set-cookie"].to_str().unwrap();
        assert!(header.contains("HttpOnly") && header.contains("SameSite=Strict"));
        let (_, _, context) = request(
            &fixture.router,
            Method::GET,
            "/api/v1/context",
            Some(&owner_cookie),
            None,
            None,
            json!({}),
        )
        .await;
        assert_eq!(context["account"]["must_change_password"], true);
        assert!(context["account"]["canonical_user_id"].is_null());
        assert_eq!(context["caps"]["owner"], false);
        let csrf = context["csrf"].as_str().unwrap();
        let (status, _, error) = request(
            &fixture.router,
            Method::POST,
            "/api/v1/models",
            Some(&owner_cookie),
            Some(csrf),
            Some(ORIGIN),
            json!({}),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(error["error"]["code"], "credential_rotation_required");
        assert_eq!(fixture.host.calls.load(Ordering::SeqCst), 0);
        let update = json!({"current_password":"owner","new_password":"changed-owner-password"});
        for (origin, token) in [(None, Some(csrf)), (Some(ORIGIN), Some("wrong-csrf"))] {
            assert_eq!(
                request(
                    &fixture.router,
                    Method::PUT,
                    "/api/v1/account/credentials",
                    Some(&owner_cookie),
                    token,
                    origin,
                    update.clone()
                )
                .await
                .0,
                StatusCode::FORBIDDEN
            );
        }
        let (status, headers, account) = request(
            &fixture.router,
            Method::PUT,
            "/api/v1/account/credentials",
            Some(&owner_cookie),
            Some(csrf),
            Some(ORIGIN),
            update,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(account["must_change_password"], false);
        let updated_cookie = cookie(&headers);
        let (_, _, context) = request(
            &fixture.router,
            Method::GET,
            "/api/v1/context",
            Some(&updated_cookie),
            None,
            None,
            json!({}),
        )
        .await;
        assert_eq!(
            context["account"]["canonical_user_id"],
            "test-owner-profile"
        );
        assert_eq!(context["caps"]["owner"], true);
        assert_eq!(context["setup"]["phase"], "model");
        let (_, _, old) = request(
            &fixture.router,
            Method::GET,
            "/api/v1/context",
            Some(&owner_cookie),
            None,
            None,
            json!({}),
        )
        .await;
        assert!(old["account"].is_null());
        assert_eq!(
            request(
                &fixture.router,
                Method::GET,
                "/api/v1/sessions?order=asc",
                Some(&updated_cookie),
                None,
                None,
                json!({})
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            request(
                &fixture.router,
                Method::GET,
                "/api/v1/sessions",
                Some(&updated_cookie),
                None,
                None,
                json!({})
            )
            .await
            .0,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn session_cookie_is_required_and_csrf_cannot_be_replaced_by_body_claims() {
        let fixture = fixture().await;
        assert_eq!(
            request(
                &fixture.router,
                Method::GET,
                "/api/v1/users",
                None,
                None,
                None,
                json!({"role":"owner"})
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            request(
                &fixture.router,
                Method::POST,
                "/api/v1/auth/credentials",
                None,
                None,
                Some("http://foreign.example"),
                json!({"username":"owner","password":"owner"})
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
        let (status, headers, _) = request(
            &fixture.router,
            Method::POST,
            "/api/v1/auth/credentials",
            None,
            None,
            Some(ORIGIN),
            json!({"username":"owner","password":"owner","role":"owner"}),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(headers.get("set-cookie").is_none());
        let (_, headers, _) = request(
            &fixture.router,
            Method::POST,
            "/api/v1/auth/credentials",
            None,
            None,
            Some(ORIGIN),
            json!({"username":"owner","password":"owner"}),
        )
        .await;
        let owner_cookie = cookie(&headers);
        let duplicate = format!("{owner_cookie}; {owner_cookie}");
        let (_, _, context) = request(
            &fixture.router,
            Method::GET,
            "/api/v1/context",
            Some(&duplicate),
            None,
            None,
            json!({}),
        )
        .await;
        assert!(context["account"].is_null());
    }

    #[tokio::test]
    async fn generated_contract_and_router_share_the_same_public_route_inventory() {
        let fixture = fixture().await;
        let response = fixture
            .router
            .clone()
            .oneshot(
                Request::get("/api/v1/openapi.json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        let document: Value = from_slice(&body).unwrap();
        assert!(document["paths"]["/sessions/{id}"]["patch"].is_object());
        assert!(document["paths"].get("/sessions/{id}/promotions").is_none());
        assert!(document["paths"].get("/account/identity-links").is_none());
        assert!(document["paths"]
            .get("/account/identity-links/{id}/confirmations")
            .is_none());
        assert!(document["paths"]["/account/profile"].get("post").is_none());
        assert!(document["paths"]["/account/profile"]["get"].is_object());
        assert!(document["paths"]["/account/identities"]["get"].is_object());
        for removed in [
            "ProfileRequest",
            "IdentityLinkRequest",
            "IdentityLinkConfirmation",
            "IdentityChallenge",
            "SessionSave",
        ] {
            assert!(document["components"]["schemas"].get(removed).is_none());
        }
        assert_eq!(
            document,
            schema(),
            "Runtime and exporter use the same schema."
        );
        assert!(
            document["components"]["schemas"]["SessionUpdate"]["properties"]["promotion_key"]
                .is_object()
        );
    }

    #[tokio::test]
    async fn sse_uses_default_data_events_and_rechecks_revocation_before_any_payload() {
        use crate::{api::models::CredentialUpdate, api::models::LoginRequest};
        let fixture = fixture().await;
        let login = fixture
            .auth
            .login(LoginRequest {
                username: "owner".into(),
                password: "owner".into(),
            })
            .await
            .unwrap();
        let owner = fixture.auth.authenticate(&login.token).await.unwrap();
        let (_, login) = fixture
            .auth
            .rotate_credentials(
                &owner,
                CredentialUpdate {
                    current_password: "owner".into(),
                    new_password: "rotated-owner-password".into(),
                    new_username: None,
                },
            )
            .await
            .unwrap();
        let owner = fixture.auth.authenticate(&login.token).await.unwrap();
        let cookie = format!("albert_auth={}", login.token);
        let response = fixture
            .router
            .clone()
            .oneshot(
                Request::get("/api/v1/sessions/owned-session/events")
                    .header("cookie", &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream"));
        let data = to_bytes(response.into_body(), 65536).await.unwrap();
        let text = String::from_utf8(data.to_vec()).unwrap();
        assert!(text.contains("data:") && text.contains("\"kind\":\"snapshot\""));
        assert!(!text.contains("event:"));
        let response = fixture
            .router
            .clone()
            .oneshot(
                Request::get("/api/v1/sessions/owned-session/events")
                    .header("cookie", &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        fixture
            .auth
            .revoke_credential(&owner, owner.credential_id())
            .await
            .unwrap();
        let data = to_bytes(response.into_body(), 65536).await.unwrap();
        assert!(
            data.is_empty(),
            "a revoked credential cannot receive queued private event data"
        );
    }
}
