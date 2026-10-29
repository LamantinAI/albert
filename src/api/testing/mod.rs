use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use async_trait::async_trait;
use axum::{
    body::{to_bytes, Body},
    http::{header::SET_COOKIE, HeaderMap, Method, Request, StatusCode},
    Router,
};
use futures::stream;
use serde_json::{from_slice, json, Value};
use sqlx::sqlite::SqlitePoolOptions;
use tempfile::{tempdir, TempDir};
use tower::ServiceExt;

use crate::{
    api::{
        errors::ApiResult,
        host::{
            EventStream, Host, HostOperation, HostResponse, ProfileOperation, SessionEventRequest,
            SessionOperation,
        },
        models::EmptyObject,
        models::{ProfileView, RuntimeStatus, SessionEvent, SessionList},
        principal::Principal,
        router as api_router,
        state::ApiState,
    },
    db::Database,
    service::auth::{AuthService, AuthSettings},
};

pub(super) struct Application {
    pub auth: Arc<AuthService>,
    pub calls: AtomicUsize,
}
#[async_trait]
impl Host for Application {
    async fn status(&self) -> ApiResult<RuntimeStatus> {
        Ok(RuntimeStatus {
            phase: "setup".into(),
            model_ready: false,
            runtime_epoch: "test-epoch".into(),
            error: None,
        })
    }
    async fn operation(
        &self,
        principal: &Principal,
        operation: HostOperation,
    ) -> ApiResult<HostResponse> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match operation {
            HostOperation::Profile(ProfileOperation::Get) => {
                let id = principal
                    .canonical_user_id()
                    .unwrap_or("test-owner-profile");
                if principal.canonical_user_id().is_none() {
                    self.auth
                        .bind_profile(principal.account_id(), id, principal.account().revision)
                        .await?;
                }
                Ok(HostResponse::Profile(ProfileView {
                    canonical_user_id: Some(id.into()),
                    display_name: None,
                }))
            }
            HostOperation::Session(SessionOperation::List(page)) => {
                assert_eq!(page.limit, Some(10));
                assert_eq!(page.order.as_deref(), Some("desc"));
                Ok(HostResponse::Sessions(SessionList {
                    items: Vec::new(),
                    next_cursor: None,
                    has_more: false,
                }))
            }
            _ => Ok(HostResponse::Empty(EmptyObject {})),
        }
    }
    async fn events(&self, _: Principal, request: SessionEventRequest) -> ApiResult<EventStream> {
        Ok(Box::pin(stream::iter([Ok(SessionEvent {
            id: "1".into(),
            session_id: request.session_id,
            runtime_epoch: "test-epoch".into(),
            kind: "snapshot".into(),
            attempt_id: None,
            data: json!({}),
        })])))
    }
}
pub(super) struct Fixture {
    pub _dir: TempDir,
    pub auth: Arc<AuthService>,
    pub host: Arc<Application>,
    pub router: Router,
}
pub(super) async fn fixture() -> Fixture {
    let dir = tempdir().unwrap();
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    Database::from_pool(pool.clone()).await.unwrap();
    let auth = AuthService::open(
        pool,
        dir.path().join("key"),
        AuthSettings {
            public_origin: "http://localhost:9877".into(),
            login_attempts: 40,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let host = Arc::new(Application {
        auth: auth.clone(),
        calls: AtomicUsize::new(0),
    });
    let router = api_router(ApiState::new(auth.clone(), host.clone()));
    Fixture {
        _dir: dir,
        auth,
        host,
        router,
    }
}
pub(super) async fn request(
    router: &Router,
    method: Method,
    path: &str,
    cookie: Option<&str>,
    csrf: Option<&str>,
    origin: Option<&str>,
    body: Value,
) -> (StatusCode, HeaderMap, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "localhost:9877")
        .header("content-type", "application/json");
    if let Some(cookie) = cookie {
        request = request.header("cookie", cookie);
    }
    if let Some(csrf) = csrf {
        request = request.header("x-albert-csrf", csrf);
    }
    if let Some(origin) = origin {
        request = request.header("origin", origin);
    }
    let response = router
        .clone()
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    let value = from_slice(&body).unwrap_or_else(|_| json!({"raw":String::from_utf8_lossy(&body)}));
    (status, headers, value)
}
pub(super) fn cookie(headers: &HeaderMap) -> String {
    headers
        .get(SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .into()
}
pub(super) const ORIGIN: &str = "http://localhost:9877";
