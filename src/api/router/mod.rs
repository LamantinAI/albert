use serde_json::from_slice;
mod account;
mod authorization;
mod resources;
mod stream;

use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, Request, State},
    http::{
        header::{CACHE_CONTROL, SET_COOKIE},
        HeaderMap, Method, StatusCode,
    },
    middleware::{from_fn, Next},
    response::{IntoResponse, Response},
    routing::{get, on, MethodFilter},
    Extension, Json, Router,
};
use serde::de::DeserializeOwned;
use std::collections::HashMap;

use self::{account::account_request, resources::host_request, stream::events};
use super::{
    docs::{schema, ROUTES},
    errors::{ApiError, ApiResult},
    extractors::{csrf, principal as authenticated_principal, same_origin, COOKIE_NAME},
    host::{HostOperation, ProfileOperation},
    models::{AccountRole, Capabilities, ClientContext, LoginRequest, SetupPhase, SetupView},
    state::ApiState,
};

#[derive(Clone, Copy)]
struct RouteSpec {
    method: &'static str,
    path: &'static str,
}

#[cfg(test)]
pub fn router(state: ApiState) -> Router {
    router_with_body_limit(state, 4 * 1024 * 1024)
}

pub fn router_with_body_limit(state: ApiState, max_body_bytes: usize) -> Router {
    let mut router = Router::new();
    for &(method, path, _, _) in ROUTES {
        let method_filter =
            MethodFilter::try_from(Method::from_bytes(method.as_bytes()).expect("static method"))
                .expect("supported method");
        router = router.route(
            &format!("/api/v1{path}"),
            on(method_filter, dispatch).layer(Extension(RouteSpec { method, path })),
        );
    }
    router
        .route("/api/v1/openapi.json", get(|| async { Json(schema()) }))
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

async fn dispatch(
    State(state): State<ApiState>,
    Extension(spec): Extension<RouteSpec>,
    Path(params): Path<HashMap<String, String>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Response> {
    if spec.path == "/context" {
        return context(&state, &headers).await;
    }
    if spec.path == "/auth/credentials" {
        same_origin(&state, &headers)?;
        let input: LoginRequest = parse(&body)?;
        let login = state.auth.login(input).await?;
        let mut response = Json(login.credential).into_response();
        set_cookie(
            &state,
            &mut response,
            &login.token,
            state.auth.settings().credential_ttl_seconds,
        )?;
        return Ok(response);
    }
    let caller = authenticated_principal(&state, &headers).await?;
    if spec.method != "GET" {
        csrf(&state, &headers, &caller)?;
    }
    let allowed_rotation = (spec.method == "PUT" && spec.path == "/account/credentials")
        || (spec.method == "DELETE" && spec.path == "/auth/credentials/{id}");
    if caller.must_rotate() && !allowed_rotation {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "credential_rotation_required",
            "Change the default credentials before continuing.",
        ));
    }
    if let Some(response) = account_request(&state, &caller, spec, &params, &body).await? {
        return Ok(response);
    }
    if spec.path.ends_with("/events") {
        return events(state, caller, &params, &query, &headers).await;
    }
    let result = host_request(&state, &caller, spec, &params, &query, &headers, &body).await?;
    Ok(Json(result).into_response())
}

async fn context(state: &ApiState, headers: &HeaderMap) -> ApiResult<Response> {
    let mut caller = match authenticated_principal(state, headers).await {
        Ok(caller) => Some(caller),
        Err(error) if error.status == StatusCode::UNAUTHORIZED => None,
        Err(error) => return Err(error),
    };
    if let Some(principal) = &caller {
        if !principal.must_rotate() && principal.canonical_user_id().is_none() {
            // Root owns the automatic configured-TG-owner/fresh-profile policy.
            state
                .host
                .operation(principal, HostOperation::Profile(ProfileOperation::Get))
                .await?;
            caller = Some(authenticated_principal(state, headers).await?);
        }
    }
    let runtime = state.host.status().await?;
    let account = caller.as_ref().map(|p| p.account().clone());
    let rotated = caller.as_ref().is_some_and(|p| !p.must_rotate());
    let owner = rotated
        && caller
            .as_ref()
            .is_some_and(|p| p.role() == AccountRole::Owner);
    let identity_required = rotated
        && caller
            .as_ref()
            .is_some_and(|p| p.canonical_user_id().is_none());
    let phase = if !rotated {
        SetupPhase::Credentials
    } else if identity_required {
        SetupPhase::Profile
    } else if runtime.error.is_some() {
        SetupPhase::RuntimeError
    } else if !runtime.model_ready {
        SetupPhase::Model
    } else {
        SetupPhase::Ready
    };
    Ok(Json(ClientContext {
        account,
        caps: Capabilities {
            owner,
            chat: rotated && !identity_required,
            manage_users: owner,
            manage_models: owner,
            manage_skills: owner,
            manage_config: owner,
        },
        setup: SetupView {
            phase,
            model_ready: runtime.model_ready,
            identity_required,
        },
        runtime_epoch: runtime.runtime_epoch,
        csrf: caller.as_ref().map(|p| p.csrf().to_owned()),
        current_credential_id: caller.as_ref().map(|p| p.credential_id().to_owned()),
    })
    .into_response())
}

fn parse<T: DeserializeOwned>(body: &[u8]) -> ApiResult<T> {
    from_slice(body)
        .map_err(|_| ApiError::invalid("The request body does not match the expected object."))
}
fn id(params: &HashMap<String, String>, key: &str) -> ApiResult<String> {
    params
        .get(key)
        .filter(|v| !v.is_empty() && v.len() <= 2048 && !v.contains('\0'))
        .cloned()
        .ok_or_else(|| ApiError::invalid("Invalid resource ID."))
}
fn set_cookie(state: &ApiState, response: &mut Response, token: &str, ttl: u64) -> ApiResult<()> {
    let secure = state.auth.settings().secure_cookie
        || state.auth.settings().public_origin.starts_with("https://");
    let cookie = format!(
        "{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={ttl}{}",
        if secure { "; Secure" } else { "" }
    );
    response.headers_mut().append(
        SET_COOKIE,
        cookie.parse().map_err(|_| ApiError::internal())?,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use axum::{
        body::{to_bytes, Body},
        http::{Method, Request, StatusCode},
    };
    use serde_json::{from_slice, json, Value};
    use tower::ServiceExt;

    use crate::api::testing::{cookie, fixture, request, ORIGIN};

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
        let published: Value =
            from_slice(include_bytes!("../../../contracts/web-panel.openapi.json")).unwrap();
        assert_eq!(
            document, published,
            "Published contract must match the actual Rust schema."
        );
        assert!(
            document["components"]["schemas"]["SessionUpdate"]["properties"]["promotion_key"]
                .is_object()
        );
    }

    #[tokio::test]
    async fn sse_uses_default_data_events_and_rechecks_revocation_before_any_payload() {
        use crate::api::models::{CredentialUpdate, LoginRequest};
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
