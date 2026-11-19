//! Shared provider protocol for ChatGPT subscription device authorization.
mod device;
mod exchange;

pub use device::{DeviceLogin, DEVICE_AUTH_TTL};
pub use exchange::ExchangeError;

use octo_openai_auth::TOKEN_URL;

use crate::error::Result;

const ISSUER: &str = "https://auth.openai.com";

/// Endpoints fixed at startup. Only the isolated local test build can override them.
#[derive(Clone)]
pub struct DeviceAuthEndpoints {
    verification_url: String,
    usercode: String,
    poll: String,
    token: String,
    redirect: String,
}

impl DeviceAuthEndpoints {
    fn new(public: &str, internal: &str) -> Self {
        Self {
            verification_url: format!("{public}/codex/device"),
            usercode: format!("{internal}/api/accounts/deviceauth/usercode"),
            poll: format!("{internal}/api/accounts/deviceauth/token"),
            token: if internal == ISSUER {
                TOKEN_URL.into()
            } else {
                format!("{internal}/oauth/token")
            },
            redirect: format!("{public}/deviceauth/callback"),
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn test_origin(origin: &str) -> Self {
        Self::new(origin, origin)
    }

    pub fn startup() -> Result<Self> {
        #[cfg(feature = "local-e2e")]
        {
            let public = local_origin("ALBERT_E2E_AUTH_PUBLIC_ORIGIN")?;
            let internal = local_origin("ALBERT_E2E_AUTH_INTERNAL_ORIGIN")?;
            Ok(Self::new(&public, &internal))
        }
        #[cfg(not(feature = "local-e2e"))]
        Ok(Self::new(ISSUER, ISSUER))
    }
}

#[cfg(feature = "local-e2e")]
fn local_origin(key: &str) -> Result<String> {
    use url::Url;

    use crate::error::Error;

    let Ok(value) = std::env::var(key) else {
        return Ok(ISSUER.into());
    };
    let invalid = || Error::Auth("Invalid local OpenAI auth origin.".into());
    let origin = Url::parse(&value).map_err(|_| invalid())?;
    if !matches!(origin.scheme(), "http" | "https")
        || origin.host_str().is_none()
        || !origin.username().is_empty()
        || origin.password().is_some()
        || origin.query().is_some()
        || origin.fragment().is_some()
        || origin.path() != "/"
    {
        return Err(invalid());
    }
    Ok(origin.origin().ascii_serialization())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        time::Duration,
    };

    use axum::{
        extract::Form, http::StatusCode, response::IntoResponse, routing::post, serve, Json, Router,
    };
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use octo_openai_auth::CLIENT_ID;
    use serde_json::{json, Value};
    use tokio::{net::TcpListener, spawn, task::JoinHandle, time::Instant};

    #[cfg(not(feature = "local-e2e"))]
    use super::ISSUER;
    use super::{DeviceAuthEndpoints, DeviceLogin};

    async fn provider(router: Router) -> (String, JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let worker = spawn(async move { serve(listener, router).await.unwrap() });
        (origin, worker)
    }

    #[cfg(not(feature = "local-e2e"))]
    #[test]
    fn production_device_endpoints_use_openai() {
        let endpoints = DeviceAuthEndpoints::startup().unwrap();
        assert_eq!(endpoints.verification_url, format!("{ISSUER}/codex/device"));
        assert_eq!(endpoints.redirect, format!("{ISSUER}/deviceauth/callback"));
        assert!(endpoints.usercode.starts_with(ISSUER));
        assert!(endpoints.poll.starts_with(ISSUER));
        assert!(endpoints.token.starts_with(ISSUER));
    }

    #[tokio::test]
    async fn device_polling_waits_and_exchanges_with_the_provider_verifier() {
        let polls = Arc::new(AtomicUsize::new(0));
        let token = URL_SAFE_NO_PAD.encode(
            json!({"https://api.openai.com/auth":{"chatgpt_account_id":"fixture-account"}})
                .to_string(),
        );
        let router = Router::new()
            .route("/api/accounts/deviceauth/usercode", post(|Json(input): Json<Value>| async move {
                assert_eq!(input, json!({"client_id": CLIENT_ID}));
                Json(json!({"device_auth_id":"private-identifier","usercode":"ABCD-EFGH","interval":"1"}))
            }))
            .route("/api/accounts/deviceauth/token", post(move |Json(input): Json<Value>| {
                let polls = polls.clone();
                async move {
                    assert_eq!(input, json!({"device_auth_id":"private-identifier","user_code":"ABCD-EFGH"}));
                    match polls.fetch_add(1, Ordering::SeqCst) {
                        0 => StatusCode::FORBIDDEN.into_response(),
                        1 => StatusCode::NOT_FOUND.into_response(),
                        2 => Json(json!({"authorization_code":"fixture-code","code_verifier":"fixture-verifier"})).into_response(),
                        _ => panic!("polled after success"),
                    }
                }
            }))
            .route("/oauth/token", post(move |Form(input): Form<HashMap<String, String>>| {
                let token = token.clone();
                async move {
                    assert_eq!(input["grant_type"], "authorization_code");
                    assert_eq!(input["client_id"], CLIENT_ID);
                    assert_eq!(input["code"], "fixture-code");
                    assert_eq!(input["code_verifier"], "fixture-verifier");
                    assert!(input["redirect_uri"].ends_with("/deviceauth/callback"));
                    Json(json!({"access_token":format!("x.{token}.x"),"refresh_token":"fixture-refresh"}))
                }
            }));
        let (origin, worker) = provider(router).await;
        let start = Instant::now();
        let login = DeviceLogin::start(
            &DeviceAuthEndpoints::test_origin(&origin),
            start + Duration::from_secs(8),
        )
        .await
        .unwrap();
        assert_eq!(login.verification_url(), format!("{origin}/codex/device"));
        assert_eq!(login.user_code(), "ABCD-EFGH");
        assert_eq!(
            login.complete().await.unwrap().account_id,
            "fixture-account"
        );
        assert!(start.elapsed() >= Duration::from_secs(2));
        worker.abort();
    }

    #[tokio::test]
    async fn expiry_stops_pending_polling_and_unavailable_issuance_is_actionable() {
        let router = Router::new()
            .route("/api/accounts/deviceauth/usercode", post(|| async {
                Json(json!({"device_auth_id":"private-id","user_code":"ABCD-EFGH","interval":300}))
            }))
            .route("/api/accounts/deviceauth/token", post(|| async { StatusCode::NOT_FOUND }));
        let (origin, worker) = provider(router).await;
        let login = DeviceLogin::start(
            &DeviceAuthEndpoints::test_origin(&origin),
            Instant::now() + Duration::from_secs(1),
        )
        .await
        .unwrap();
        assert!(login.complete().await.err().unwrap().expired());
        worker.abort();
        let (origin, worker) = provider(Router::new()).await;
        let err = DeviceLogin::start(
            &DeviceAuthEndpoints::test_origin(&origin),
            Instant::now() + Duration::from_secs(5),
        )
        .await
        .err()
        .unwrap();
        assert_eq!(err.http_status(), Some(404));
        assert!(err.public_message().contains("ChatGPT security settings"));
        worker.abort();
    }
}
