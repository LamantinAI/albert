//! Optional account quota observations. Failures never change inference readiness.
mod parse;

use std::{time::Duration, time::Instant};

use chrono::Utc;
use octo_openai_auth::{Subscription, SubscriptionAuth};
use reqwest::{redirect::Policy, Client, Url};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use self::parse::parse_windows;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LimitWindow {
    pub bucket: String,
    pub duration_seconds: u64,
    pub remaining_percent: f64,
    pub resets_at: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LimitsSnapshot {
    pub state: String,
    pub observed_at: Option<String>,
    pub windows: Vec<LimitWindow>,
}
impl LimitsSnapshot {
    fn empty(state: &str) -> Self {
        Self {
            state: state.into(),
            observed_at: None,
            windows: vec![],
        }
    }
}
struct Cached {
    key: String,
    at: Instant,
    result: LimitsSnapshot,
}
#[derive(Default)]
pub struct SubscriptionLimits {
    cache: Mutex<Option<Cached>>,
}

fn identity(sub: &Subscription, base: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(sub.account_id.as_bytes());
    hash.update([0]);
    hash.update(sub.access_token.as_bytes());
    hash.update([0]);
    hash.update(base.as_bytes());
    format!("{:x}", hash.finalize())
}
fn endpoint(base: &str) -> Option<Url> {
    let mut url = Url::parse(base).ok()?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    if url.scheme() != "https"
        && !(url.scheme() == "http"
            && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")))
    {
        return None;
    }
    let prefix = url
        .path()
        .trim_end_matches('/')
        .strip_suffix("/backend-api/codex")?
        .to_owned();
    url.set_path(&format!("{prefix}/backend-api/wham/usage"));
    Some(url)
}
impl SubscriptionLimits {
    pub async fn get(&self, auth: &SubscriptionAuth, base: &str) -> LimitsSnapshot {
        let Some(url) = endpoint(base) else {
            return LimitsSnapshot::empty("unsupported_endpoint");
        };
        let Ok(sub) = auth.fresh().await else {
            *self.cache.lock().await = None;
            return LimitsSnapshot::empty("authentication_required");
        };
        let key = identity(&sub, base);
        // This lock is independent of the credential refresh lock; one request per account cache.
        let mut cache = self.cache.lock().await;
        if let Some(cached) = cache
            .as_ref()
            .filter(|c| c.key == key && c.at.elapsed() < Duration::from_secs(60))
        {
            let result = cached.result.clone();
            if auth
                .fresh()
                .await
                .ok()
                .is_some_and(|current| identity(&current, base) == key)
            {
                return result;
            }
            *cache = None;
            return LimitsSnapshot::empty("authentication_required");
        }
        if cache.as_ref().is_some_and(|c| c.key != key) {
            *cache = None;
        }
        let result = fetch(url, &sub).await;
        // An in-flight response cannot survive account replacement or logout.
        if auth
            .fresh()
            .await
            .ok()
            .is_none_or(|current| identity(&current, base) != key)
        {
            *cache = None;
            return LimitsSnapshot::empty("authentication_required");
        }
        let result = match result {
            Ok(windows) => LimitsSnapshot {
                state: if windows.is_empty() {
                    "not_reported"
                } else {
                    "available"
                }
                .into(),
                observed_at: Some(Utc::now().to_rfc3339()),
                windows,
            },
            Err(state) => match cache.as_ref().filter(|c| {
                c.at.elapsed() < Duration::from_secs(300) && state != "authentication_required"
            }) {
                Some(cached) => {
                    let mut result = cached.result.clone();
                    result.state = "stale".into();
                    return result;
                }
                None => LimitsSnapshot::empty(state),
            },
        };
        *cache = Some(Cached {
            key,
            at: Instant::now(),
            result: result.clone(),
        });
        result
    }
}
async fn fetch(url: Url, sub: &Subscription) -> Result<Vec<LimitWindow>, &'static str> {
    let client = Client::builder()
        .redirect(Policy::none())
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| "unavailable")?;
    let mut response = client
        .get(url)
        .bearer_auth(&sub.access_token)
        .header("ChatGPT-Account-ID", &sub.account_id)
        .send()
        .await
        .map_err(|_| "unavailable")?;
    if matches!(response.status().as_u16(), 401 | 403) {
        return Err("authentication_required");
    }
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|size| size > 1_048_576)
    {
        return Err("unavailable");
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "unavailable")? {
        if body.len() + chunk.len() > 1_048_576 {
            return Err("unavailable");
        }
        body.extend_from_slice(&chunk);
    }
    parse_windows(&body).ok_or("unavailable")
}

#[cfg(test)]
mod tests {
    use super::{endpoint, parse::parse_windows};
    use serde_json::{json, to_vec};
    #[test]
    fn quota_endpoint_is_sibling_and_preserves_proxy_prefix() {
        assert_eq!(
            endpoint("https://provider.example/proxy/backend-api/codex/")
                .unwrap()
                .as_str(),
            "https://provider.example/proxy/backend-api/wham/usage"
        );
        assert!(endpoint("https://provider.example/v1").is_none());
        assert!(endpoint("https://user:secret@provider.example/backend-api/codex").is_none());
        assert!(endpoint("https://provider.example/backend-api/codex?token=secret").is_none());
    }

    #[test]
    fn windows_validate_reported_data_and_preserve_shared_buckets() {
        let valid = json!({"used_percent":27,"limit_window_seconds":18000,"reset_at":1900000000});
        let value = json!({"rate_limit":{"primary_window":valid,"secondary_window":{"used_percent":101,"limit_window_seconds":604800,"reset_at":1900000000}},"additional_rate_limits":[{"limit_name":"Review","rate_limit":{"primary_window":{"used_percent":0,"limit_window_seconds":3600,"reset_at":1900000000}}}]});
        let windows = parse_windows(&to_vec(&value).unwrap()).unwrap();
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].remaining_percent, 73.0);
        assert_eq!(windows[0].bucket, "Subscription");
        assert_eq!(windows[1].bucket, "Review");
        assert_eq!(windows[1].remaining_percent, 100.0);
        assert!(parse_windows(b"{}").unwrap().is_empty());
        assert!(parse_windows(b"[]").is_none());
        assert!(
            parse_windows(br#"{"rate_limit":{"primary_window":{"used_percent":0}}}"#)
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn account_cache_is_single_flight_and_replacement_cannot_reuse_old_limits() {
        use super::SubscriptionLimits;
        use axum::{http::HeaderMap, routing::get, serve, Json, Router};
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        use chrono::Utc;
        use octo_openai_auth::{SubscriptionAuth, Tokens};
        use std::sync::{Arc, Mutex};
        use tempfile::tempdir;
        use tokio::{join, net::TcpListener, spawn};
        let seen = Arc::new(Mutex::new(Vec::new()));
        let observed = seen.clone();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!(
            "http://{}/gateway/backend-api/codex",
            listener.local_addr().unwrap()
        );
        let router=Router::new().route("/gateway/backend-api/wham/usage",get(move |headers:HeaderMap|{
            let seen=observed.clone();async move {
                let account=headers["chatgpt-account-id"].to_str().unwrap().to_owned();
                assert!(headers["authorization"].to_str().unwrap().starts_with("Bearer "));
                seen.lock().unwrap().push(account.clone());
                Json(json!({"rate_limit":{"primary_window":{"used_percent":if account=="account-a"{20}else{60},"limit_window_seconds":18000,"reset_at":1900000000}}}))
            }
        }));
        let server = spawn(async move { serve(listener, router).await.unwrap() });
        let dir = tempdir().unwrap();
        let auth = SubscriptionAuth::new(dir.path().join("auth.json"));
        let payload =
            URL_SAFE_NO_PAD.encode(json!({"exp":Utc::now().timestamp()+3600}).to_string());
        let token = format!("e30.{payload}.signature");
        let install = |account: &str| Tokens {
            access_token: token.clone(),
            refresh_token: "test-refresh".into(),
            id_token: String::new(),
            account_id: account.into(),
        };
        auth.install(install("account-a")).await.unwrap();
        let limits = SubscriptionLimits::default();
        let (a, b) = join!(limits.get(&auth, &base), limits.get(&auth, &base));
        assert_eq!(a.windows[0].remaining_percent, 80.0);
        assert_eq!(b.windows[0].remaining_percent, 80.0);
        assert_eq!(seen.lock().unwrap().as_slice(), ["account-a"]);
        auth.install(install("account-b")).await.unwrap();
        let replaced = limits.get(&auth, &base).await;
        assert_eq!(replaced.windows[0].remaining_percent, 40.0);
        assert_eq!(seen.lock().unwrap().as_slice(), ["account-a", "account-b"]);
        server.abort();
    }
}
