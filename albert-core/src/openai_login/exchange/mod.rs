mod diagnostics;
pub use diagnostics::ExchangeError;

use std::time::Duration;

use octo_openai_auth::{account_id_from_jwt, Tokens, CLIENT_ID};
use reqwest::{redirect::Policy, Client, Error as HttpError};
use serde::Deserialize;
use serde_json::{from_slice, Value};

use self::diagnostics::Category;

const MAX_RESPONSE: usize = 64 * 1024;

pub(super) async fn exchange_code(
    endpoint: &str,
    redirect: &str,
    verifier: &str,
    code: &str,
) -> Result<Tokens, ExchangeError> {
    let client = Client::builder()
        .timeout(Duration::from_secs(20))
        .connect_timeout(Duration::from_secs(5))
        .redirect(Policy::none())
        .build()
        .map_err(|_| ExchangeError::new(Category::ClientSetup, None))?;
    exchange_using(&client, endpoint, redirect, verifier, code).await
}
async fn exchange_using(
    client: &Client,
    endpoint: &str,
    redirect: &str,
    verifier: &str,
    code: &str,
) -> Result<Tokens, ExchangeError> {
    let mut response = client
        .post(endpoint)
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", CLIENT_ID),
            ("code", code),
            ("redirect_uri", redirect),
            ("code_verifier", verifier),
        ])
        .send()
        .await
        .map_err(|error| transport(&error, None))?;
    let status = response.status();
    let http_status = Some(status.as_u16());
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE as u64)
    {
        return Err(ExchangeError::new(Category::ResponseTooLarge, http_status));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        ExchangeError::new(
            if error.is_timeout() {
                Category::Timeout
            } else {
                Category::ResponseRead
            },
            http_status,
        )
    })? {
        if body.len() + chunk.len() > MAX_RESPONSE {
            return Err(ExchangeError::new(Category::ResponseTooLarge, http_status));
        }
        body.extend_from_slice(&chunk);
    }
    if !status.is_success() {
        let parsed: Option<Value> = from_slice(&body).ok();
        let error = parsed.as_ref().and_then(|value| value.get("error"));
        let code = error
            .and_then(Value::as_str)
            .or_else(|| {
                error
                    .and_then(|value| value.get("code"))
                    .and_then(Value::as_str)
            })
            .or_else(|| {
                error
                    .and_then(|value| value.get("type"))
                    .and_then(Value::as_str)
            });
        return Err(ExchangeError::rejected(status.as_u16(), code));
    }
    decode_tokens(&body, http_status)
}
fn transport(error: &HttpError, status: Option<u16>) -> ExchangeError {
    ExchangeError::new(
        if error.is_timeout() {
            Category::Timeout
        } else if error.is_connect() {
            Category::Connect
        } else {
            Category::Transport
        },
        status,
    )
}
#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: String,
}
pub(super) fn decode_tokens(body: &[u8], status: Option<u16>) -> Result<Tokens, ExchangeError> {
    let response: TokenResponse =
        from_slice(body).map_err(|_| ExchangeError::new(Category::ResponseJson, status))?;
    let access_token = response
        .access_token
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| ExchangeError::new(Category::MissingCredentials, status))?;
    let refresh_token = response
        .refresh_token
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| ExchangeError::new(Category::MissingCredentials, status))?;
    let account_id = account_id_from_jwt(&response.id_token)
        .or_else(|| account_id_from_jwt(&access_token))
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| ExchangeError::new(Category::MissingAccount, status))?;
    Ok(Tokens {
        access_token,
        refresh_token,
        id_token: response.id_token,
        account_id,
    })
}
pub(super) fn invalid_code() -> ExchangeError {
    ExchangeError::new(Category::InvalidCode, None)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use octo_openai_auth::Tokens;
    use reqwest::Client;
    use tokio::{
        io::AsyncReadExt, io::AsyncWriteExt, net::TcpListener, spawn, sync::Notify, time::timeout,
    };

    use super::{exchange_using, ExchangeError};

    const SENTINEL: &str = "private-response-sentinel";

    fn assert_safe(error: &ExchangeError) {
        let exposed = format!(
            "{} {error:?} {} {:?} {:?}",
            error,
            error.category(),
            error.http_status(),
            error.oauth_code()
        );
        for secret in [
            SENTINEL,
            "private-code-sentinel",
            "private-verifier-sentinel",
            "http://",
            "https://",
        ] {
            assert!(
                !exposed.contains(secret),
                "diagnostic fields must contain only static values"
            );
        }
    }
    fn failure(result: Result<Tokens, ExchangeError>) -> ExchangeError {
        match result {
            Err(error) => error,
            Ok(_) => panic!("expected a rejected exchange"),
        }
    }
    fn client() -> Client {
        Client::builder()
            .no_proxy()
            .timeout(Duration::from_millis(150))
            .build()
            .unwrap()
    }
    async fn response(status: u16, body: &str, declared_length: usize) -> ExchangeError {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!(
            "http://{}/private-endpoint-sentinel",
            listener.local_addr().unwrap()
        );
        let wire = format!("HTTP/1.1 {status} Result\r\nContent-Length: {declared_length}\r\nConnection: close\r\n\r\n{body}");
        let server = spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 8192];
            socket.read(&mut request).await.unwrap();
            let _ = socket.write_all(wire.as_bytes()).await;
        });
        let error = failure(
            exchange_using(
                &client(),
                &endpoint,
                "http://localhost:1455/auth/callback",
                "private-verifier-sentinel",
                "private-code-sentinel",
            )
            .await,
        );
        server.await.unwrap();
        assert_safe(&error);
        assert_eq!(error.http_status(), Some(status));
        error
    }

    #[tokio::test]
    async fn rejected_status_and_allowlisted_codes_are_actionable_and_safe() {
        timeout(Duration::from_secs(3), async {
            for (status, body, code, message) in [
                (400, r#"{"error":"invalid_grant","error_description":"private-response-sentinel"}"#, Some("invalid_grant"), "already used"),
                (403, r#"{"error":{"code":"unsupported_country_region_territory","message":"private-response-sentinel"}}"#, Some("unsupported_country_region_territory"), "unsupported country or region"),
                (401, r#"{"error":{"type":"token_expired","message":"private-response-sentinel"}}"#, Some("token_expired"), "expired"),
                (429, r#"{"error":"private-response-sentinel","error_description":"private-response-sentinel"}"#, None, "rate-limited"),
                (502, "private-response-sentinel", None, "temporarily unable"),
                (400, r#"{"error":"invalid_client","error_description":"private-response-sentinel"}"#, Some("invalid_client"), "client configuration"),
            ] {
                let error = response(status, body, body.len()).await;
                assert_eq!(error.category(), "http_rejected");
                assert_eq!(error.oauth_code(), code);
                assert!(error.public_message().contains(message));
            }
        }).await.expect("bounded synthetic rejections");
    }
    #[tokio::test]
    async fn invalid_success_read_and_large_responses_remain_distinct_and_safe() {
        timeout(Duration::from_secs(3), async {
            for (body, category) in [
                ("private-response-sentinel", "response_json"),
                (r#"{"access_token":"private-response-sentinel"}"#, "missing_credentials"),
                (r#"{"access_token":"private-response-sentinel","refresh_token":"private-response-sentinel"}"#, "missing_account"),
            ] {
                assert_eq!(response(200, body, body.len()).await.category(), category);
            }
            assert_eq!(response(200, SENTINEL, 65_537).await.category(), "response_too_large");
            assert_eq!(response(200, SENTINEL, SENTINEL.len() + 100).await.category(), "response_read");
        }).await.expect("bounded malformed responses");
    }
    #[tokio::test]
    async fn transport_connect_and_timeout_drop_request_details() {
        timeout(Duration::from_secs(3), async {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!(
                "http://{}/private-endpoint-sentinel",
                listener.local_addr().unwrap()
            );
            drop(listener);
            let error = failure(
                exchange_using(
                    &client(),
                    &endpoint,
                    "http://localhost/",
                    "private-verifier-sentinel",
                    "private-code-sentinel",
                )
                .await,
            );
            assert_safe(&error);
            assert_eq!(error.category(), "transport_connect");
            assert_eq!(error.http_status(), None);
            for headers in [false, true] {
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let endpoint = format!(
                    "http://{}/private-endpoint-sentinel",
                    listener.local_addr().unwrap()
                );
                let server = spawn(async move {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let mut request = [0u8; 8192];
                    socket.read(&mut request).await.unwrap();
                    if headers {
                        socket
                            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n")
                            .await
                            .unwrap();
                    }
                    Notify::new().notified().await;
                });
                let error = failure(
                    exchange_using(
                        &client(),
                        &endpoint,
                        "http://localhost/",
                        "private-verifier-sentinel",
                        "private-code-sentinel",
                    )
                    .await,
                );
                assert_safe(&error);
                assert_eq!(error.category(), "transport_timeout");
                assert_eq!(error.http_status(), if headers { Some(200) } else { None });
                server.abort();
            }
        })
        .await
        .expect("bounded synthetic transport failures");
    }
}
