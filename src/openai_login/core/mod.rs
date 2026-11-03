mod exchange;
pub(crate) use exchange::ExchangeError;

use std::{collections::HashSet, result::Result as StdResult};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use octo_openai_auth::{Tokens, CLIENT_ID, TOKEN_URL};
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};
use url::Url;

use self::exchange::{exchange_code, invalid_code};
use crate::error::{Error, Result};

pub(crate) const REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const AUTHORIZE_URL: &str = "https://auth.openai.com/oauth/authorize";
const MAX_INPUT: usize = 8192;

/// Fixed endpoints captured by the assembly at startup, never supplied by a request.
#[derive(Clone)]
pub(crate) struct OAuthEndpoints {
    authorize: String,
    token: String,
}
impl OAuthEndpoints {
    #[cfg(test)]
    pub(crate) fn test_endpoint(token: String) -> Self {
        Self {
            authorize: AUTHORIZE_URL.into(),
            token,
        }
    }
    pub fn startup() -> Result<Self> {
        #[cfg(feature = "local-e2e")]
        {
            use std::env::var;
            let endpoint = |key: &str, path: &str, fallback: &str| -> Result<String> {
                match var(key) {
                    Ok(value) => {
                        let origin = Url::parse(&value)
                            .map_err(|_| invalid("Invalid local OAuth origin."))?;
                        if !matches!(origin.scheme(), "http" | "https")
                            || origin.host_str().is_none()
                            || !origin.username().is_empty()
                            || origin.password().is_some()
                            || origin.query().is_some()
                            || origin.fragment().is_some()
                            || origin.path() != "/"
                        {
                            return Err(invalid("Invalid local OAuth origin."));
                        }
                        Ok(format!("{}{path}", value.trim_end_matches('/')))
                    }
                    Err(_) => Ok(fallback.into()),
                }
            };
            return Ok(Self {
                authorize: endpoint(
                    "ALBERT_E2E_AUTH_PUBLIC_ORIGIN",
                    "/oauth/authorize",
                    AUTHORIZE_URL,
                )?,
                token: endpoint("ALBERT_E2E_AUTH_INTERNAL_ORIGIN", "/oauth/token", TOKEN_URL)?,
            });
        }
        #[cfg(not(feature = "local-e2e"))]
        Ok(Self {
            authorize: AUTHORIZE_URL.into(),
            token: TOKEN_URL.into(),
        })
    }
}

/// Owns the PKCE secret and state for exactly one authorization attempt.
pub(crate) struct LoginFlow {
    verifier: String,
    state: String,
    redirect: String,
    authorize: String,
    token: String,
}
impl LoginFlow {
    pub fn new(endpoints: &OAuthEndpoints) -> Self {
        Self::for_redirect(endpoints, REDIRECT_URI)
    }

    pub(super) fn for_redirect(endpoints: &OAuthEndpoints, redirect: &str) -> Self {
        let verifier = random_b64(32);
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let state = random_b64(24);
        let mut authorize = Url::parse(&endpoints.authorize).expect("validated OAuth endpoint");
        authorize
            .query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", CLIENT_ID)
            .append_pair("redirect_uri", redirect)
            .append_pair("scope", "openid profile email offline_access")
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &state)
            .append_pair("id_token_add_organizations", "true")
            .append_pair("codex_cli_simplified_flow", "true")
            .append_pair("originator", "codex_cli_rs");
        Self {
            verifier,
            state,
            redirect: redirect.into(),
            authorize: authorize.into(),
            token: endpoints.token.clone(),
        }
    }
    pub fn authorize_url(&self) -> &str {
        &self.authorize
    }
    pub(super) fn callback_origin(&self) -> String {
        Url::parse(&self.redirect)
            .expect("fixed redirect")
            .origin()
            .ascii_serialization()
    }

    /// Accept a full callback URL, legacy query paste, or a bare PKCE-bound code.
    /// This only parses input; it never fetches the pasted URL.
    pub fn parse_submission(&self, input: &str) -> Result<Option<String>> {
        let input = input.trim();
        if input.is_empty() {
            return Ok(None);
        }
        if input.len() > MAX_INPUT || input.chars().any(char::is_control) {
            return Err(invalid("Authorization input is too long or malformed."));
        }
        let url = if input.contains("://") {
            let url = Url::parse(input).map_err(|_| invalid("Invalid callback URL."))?;
            let expected = Url::parse(&self.redirect).expect("fixed redirect");
            if url.origin() != expected.origin()
                || url.path() != expected.path()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.fragment().is_some()
            {
                return Err(invalid(
                    "Use the complete localhost authorization callback URL.",
                ));
            }
            url
        } else if input.starts_with("code=")
            || input.starts_with("state=")
            || input.starts_with("error=")
        {
            Url::parse(&format!("{}?{input}", self.redirect))
                .map_err(|_| invalid("Invalid callback query."))?
        } else {
            return valid_code(input).map(|_| Some(input.into()));
        };
        let query = url
            .query()
            .ok_or_else(|| invalid("Callback URL has no authorization result."))?;
        let bytes = query.as_bytes();
        for (at, byte) in bytes.iter().enumerate() {
            if *byte == b'%'
                && (at + 2 >= bytes.len()
                    || !bytes[at + 1].is_ascii_hexdigit()
                    || !bytes[at + 2].is_ascii_hexdigit())
            {
                return Err(invalid("Malformed callback encoding."));
            }
        }
        let mut seen = HashSet::new();
        let (mut code, mut state, mut error) = (None, None, false);
        for (key, value) in url.query_pairs() {
            if !seen.insert(key.to_string())
                || value.contains('\u{fffd}')
                || value.chars().any(char::is_control)
            {
                return Err(invalid("Duplicate or malformed callback parameter."));
            }
            match key.as_ref() {
                "code" => code = Some(value.into_owned()),
                "state" => state = Some(value.into_owned()),
                "error" => error = true,
                _ => {}
            }
        }
        if state.as_deref() != Some(self.state.as_str()) {
            return Err(invalid(
                "Callback state does not match this authorization attempt.",
            ));
        }
        if error {
            return Err(invalid(
                "The provider declined authorization. Start a new attempt.",
            ));
        }
        let code = code.ok_or_else(|| invalid("Callback contains no authorization code."))?;
        valid_code(&code)?;
        Ok(Some(code))
    }

    pub async fn exchange(&self, code: &str) -> StdResult<Tokens, ExchangeError> {
        valid_code(code).map_err(|_| invalid_code())?;
        exchange_code(&self.token, &self.redirect, &self.verifier, code).await
    }
}

fn valid_code(code: &str) -> Result<()> {
    if code.is_empty()
        || code.len() > 4096
        || !code
            .bytes()
            .all(|b| b.is_ascii_graphic() && !b"&?#=%".contains(&b))
    {
        return Err(invalid("Authorization code is empty or malformed."));
    }
    Ok(())
}
fn invalid(message: &str) -> Error {
    Error::Auth(message.into())
}
fn random_b64(size: usize) -> String {
    let mut bytes = vec![0; size];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}
#[cfg(test)]
mod tests {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use serde_json::json;
    use sha2::{Digest, Sha256};
    use url::Url;

    use super::{exchange::decode_tokens, LoginFlow, OAuthEndpoints, REDIRECT_URI};

    #[cfg(not(feature = "local-e2e"))]
    #[test]
    fn production_build_always_uses_public_provider_endpoints() {
        use super::AUTHORIZE_URL;
        use octo_openai_auth::TOKEN_URL;
        let endpoints = OAuthEndpoints::startup().unwrap();
        assert_eq!(endpoints.authorize, AUTHORIZE_URL);
        assert_eq!(endpoints.token, TOKEN_URL);
    }

    fn make_flow() -> LoginFlow {
        LoginFlow::new(&OAuthEndpoints {
            authorize: "https://auth.example.test/oauth/authorize".into(),
            token: "https://auth.example.test/oauth/token".into(),
        })
    }
    fn callback(flow: &LoginFlow, query: &str) -> String {
        format!("{REDIRECT_URI}?state={}&{query}", flow.state)
    }

    #[test]
    fn authorization_uses_s256_unique_state_and_exact_redirect() {
        let flow = make_flow();
        let url = Url::parse(flow.authorize_url()).unwrap();
        let params: Vec<_> = url.query_pairs().collect();
        let get = |name: &str| {
            params
                .iter()
                .find(|(key, _)| key == name)
                .unwrap()
                .1
                .as_ref()
        };
        assert_eq!(get("redirect_uri"), REDIRECT_URI);
        assert_eq!(get("code_challenge_method"), "S256");
        assert_eq!(
            get("code_challenge"),
            URL_SAFE_NO_PAD.encode(Sha256::digest(flow.verifier.as_bytes()))
        );
        assert!(!flow.authorize_url().contains(&flow.verifier));
        assert_ne!(flow.state, make_flow().state);
    }
    #[test]
    fn callback_and_legacy_query_and_bare_code_work() {
        let flow = make_flow();
        for input in [
            callback(&flow, "code=abc&scope=openid"),
            format!("code=abc&state={}", flow.state),
            "abc".into(),
        ] {
            assert_eq!(
                flow.parse_submission(&input).unwrap().as_deref(),
                Some("abc")
            );
        }
        assert!(flow.parse_submission(" ").unwrap().is_none());
    }
    #[test]
    fn rejects_wrong_origin_path_state_duplicates_errors_and_bad_encoding() {
        let flow = make_flow();
        let good = callback(&flow, "code=abc");
        for input in [
            good.replace("localhost", "example.test"),
            good.replace(":1455", ":1457"),
            good.replace("/auth/callback", "/auth/callback/extra"),
            good.replace("http:", "https:"),
            format!("{good}#fragment"),
            good.replace("localhost", "user@localhost"),
            callback(&flow, "code=abc&code=xyz"),
            callback(&flow, "code=abc&state=other"),
            callback(&flow, "code=abc&error=access_denied"),
            callback(&flow, "error=access_denied"),
            callback(&flow, "code=%GG"),
            callback(&flow, "code=%FF"),
            callback(&flow, "code="),
            format!("{REDIRECT_URI}?code=abc&state=wrong"),
            "bad code".into(),
            "https://example.test".into(),
        ] {
            assert!(
                flow.parse_submission(&input).is_err(),
                "must reject malformed callback"
            );
        }
    }
    #[test]
    fn token_response_requires_refresh_access_and_account_without_echoing_secrets() {
        for body in [
            br#"{"access_token":"secret"}"#.as_slice(),
            br#"{"access_token":"secret","refresh_token":"secret"}"#,
            b"secret-invalid-json",
        ] {
            let error = match decode_tokens(body, Some(200)) {
                Err(error) => error.to_string(),
                Ok(_) => panic!("invalid"),
            };
            assert!(!error.contains("secret"));
        }
        let payload = URL_SAFE_NO_PAD.encode(
            json!({"https://api.openai.com/auth":{"chatgpt_account_id":"synthetic-account"}})
                .to_string(),
        );
        let decoded = decode_tokens(
            json!({"access_token":format!("x.{payload}.x"),"refresh_token":"synthetic-refresh"})
                .to_string()
                .as_bytes(),
            Some(200),
        )
        .unwrap();
        assert_eq!(decoded.account_id, "synthetic-account");
    }
    #[tokio::test]
    async fn exchange_bounds_responses_and_never_echoes_provider_body() {
        use axum::{http::StatusCode, routing::post, serve, Router};
        use std::time::Duration;
        use tokio::{net::TcpListener, spawn, time::timeout};
        timeout(Duration::from_secs(5), async {
            for (status, body) in [
                (400, "private-provider-body".to_owned()),
                (200, "private-provider-body".to_owned()),
                (200, "x".repeat(65_537)),
            ] {
                let router = Router::new().route(
                    "/token",
                    post(move || {
                        let body = body.clone();
                        async move { (StatusCode::from_u16(status).unwrap(), body) }
                    }),
                );
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let mut flow = make_flow();
                flow.token = format!("http://{}/token", listener.local_addr().unwrap());
                let server = spawn(async move {
                    serve(listener, router).await.unwrap();
                });
                let error = match flow.exchange("synthetic-code").await {
                    Err(error) => error.to_string(),
                    Ok(_) => panic!("must reject response"),
                };
                assert!(!error.contains("private-provider-body"));
                server.abort();
            }
        })
        .await
        .expect("bounded exchange validation");
    }
}
