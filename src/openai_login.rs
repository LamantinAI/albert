//! `albert login` — the interactive ChatGPT-**subscription** sign-in.
//!
//! An OAuth 2.0 Authorization-Code + PKCE flow against `auth.openai.com`, using the
//! first-party Codex client. It spins up a throwaway loopback server to catch the
//! redirect, opens the browser, exchanges the code for tokens, and writes them to
//! the same codex-style `auth.json` the runtime reads ([`crate::openai_auth`]) — so
//! Albert no longer depends on the `codex` CLI being installed or logged in.

use std::{path::Path, process::Command, time::Duration};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::{rngs::OsRng, RngCore};
use reqwest::Client as HttpClient;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::{
    io::{stdin, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    time::timeout,
};
use tracing::warn;
use url::Url;

use crate::{
    error::{Error, Result},
    openai_auth::{account_id_from_jwt, plan_from_jwt, AuthDotJson, Tokens, CLIENT_ID, TOKEN_URL},
};

const AUTHORIZE_URL: &str = "https://auth.openai.com/oauth/authorize";
const SCOPE: &str = "openid profile email offline_access";
/// The loopback ports the redirect may land on (Codex's primary + fallback).
const PORTS: [u16; 2] = [1455, 1457];

/// Run the full login flow and persist the resulting tokens to `store_path`.
pub async fn run(store_path: &Path) -> Result<()> {
    let (listener, port) = bind_callback().await?;
    let redirect_uri = format!("http://localhost:{port}/auth/callback");

    // PKCE (S256) + an anti-CSRF state.
    let verifier = random_b64(32);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let state = random_b64(24);

    let authorize = build_authorize_url(&redirect_uri, &challenge, &state);
    println!(
        "\nSign in with your ChatGPT account — open this URL in your browser:\n\n  {authorize}\n\n\
         Waiting for the redirect on {redirect_uri} ...\n"
    );
    if let Err(e) = open_browser(&authorize) {
        warn!(error = %e, "could not open a browser automatically; open the URL above manually");
    }

    let code = await_code(&listener, &state).await?;
    let tokens = exchange_code(&code, &redirect_uri, &verifier).await?;

    let account_id = account_id_from_jwt(&tokens.id_token).unwrap_or_default();
    let plan = plan_from_jwt(&tokens.access_token);
    let store = AuthDotJson::new(Tokens {
        id_token: tokens.id_token,
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        account_id,
    });
    store.save(store_path)?;

    let plan = plan.map(|p| format!(" (plan: {p})")).unwrap_or_default();
    println!("Signed in{plan}. Tokens saved to {}.", store_path.display());
    Ok(())
}

/// Bind the first available loopback port from [`PORTS`].
async fn bind_callback() -> Result<(TcpListener, u16)> {
    for port in PORTS {
        if let Ok(listener) = TcpListener::bind(("127.0.0.1", port)).await {
            return Ok((listener, port));
        }
    }
    Err(Error::Auth(format!("could not bind any login callback port {PORTS:?}")))
}

fn build_authorize_url(redirect_uri: &str, challenge: &str, state: &str) -> String {
    let mut url = Url::parse(AUTHORIZE_URL).expect("static authorize url");
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", CLIENT_ID)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("scope", SCOPE)
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state)
        .append_pair("id_token_add_organizations", "true")
        .append_pair("codex_cli_simplified_flow", "true")
        .append_pair("originator", "codex_cli_rs");
    url.into()
}

/// Wait for the authorization `code` two ways at once, whichever lands first:
///
/// - the browser redirect hits the loopback callback server (works when the browser
///   and Albert share a machine), or
/// - the user pastes the redirect URL (or the bare code) on stdin — the headless /
///   remote-VM fallback, since a browser elsewhere can't reach this box's loopback.
async fn await_code(listener: &TcpListener, state: &str) -> Result<String> {
    println!("...or paste the redirect URL (or just the code) here and press Enter.\n");
    let mut lines = BufReader::new(stdin()).lines();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (mut sock, _) = accepted
                    .map_err(|e| Error::Auth(format!("callback accept failed: {e}")))?;
                if let Some(code) = handle_callback(&mut sock, state).await? {
                    return Ok(code);
                }
            }
            line = lines.next_line() => match line {
                Ok(Some(l)) => {
                    if let Some(code) = parse_pasted(l.trim(), state)? {
                        return Ok(code);
                    }
                }
                Ok(None) => {} // stdin closed — keep waiting on the socket
                Err(e) => return Err(Error::Auth(format!("stdin read failed: {e}"))),
            },
        }
    }
}

/// Handle one loopback request. `Some(code)` on a valid callback, `None` for an
/// unrelated request (favicon, etc.), `Err` on an OAuth error or a state mismatch.
async fn handle_callback(sock: &mut TcpStream, state: &str) -> Result<Option<String>> {
    let Some(path) = request_target(sock).await else { return Ok(None) };
    if !path.starts_with("/auth/callback") {
        let _ = respond(sock, "404 Not Found", "").await;
        return Ok(None);
    }
    let url = Url::parse(&format!("http://localhost{path}"))
        .map_err(|e| Error::Auth(format!("bad callback url: {e}")))?;
    let (code, got_state, oauth_err) = params(&url);
    // A request whose state doesn't match this session is stray or hostile — ignore
    // it and keep waiting, so no unauthenticated local caller can abort the login by
    // hitting the loopback port. Only a state-MATCHING request can end the flow.
    if got_state.as_deref() != Some(state) {
        let _ = respond(sock, "400 Bad Request", &page("Ignoring an unrecognized request.")).await;
        return Ok(None);
    }
    if let Some(err) = oauth_err {
        let _ = respond(sock, "400 Bad Request", &page("Sign-in failed.")).await;
        return Err(Error::Auth(format!("authorization error: {err}")));
    }
    let Some(code) = code else {
        let _ = respond(sock, "400 Bad Request", &page("Sign-in failed (no code).")).await;
        return Err(Error::Auth("callback carried no authorization code".into()));
    };
    let _ = respond(sock, "200 OK", &page("Signed in. You can close this tab and return to Albert.")).await;
    Ok(Some(code))
}

/// Parse a pasted line: a full redirect URL (`...?code=..&state=..`, state checked)
/// or a bare authorization code (accepted as-is — PKCE binds it to this session, so
/// a mismatched code just fails the exchange). `None` for an empty line.
fn parse_pasted(line: &str, state: &str) -> Result<Option<String>> {
    if line.is_empty() {
        return Ok(None);
    }
    if line.contains("code=") {
        // A full redirect URL parses directly; a bare (scheme-less) query string is
        // wrapped so its `code`/`state` land in the query rather than the path.
        let url = Url::parse(line).ok().or_else(|| {
            let qs = line.split_once('?').map(|(_, q)| q).unwrap_or(line);
            Url::parse(&format!("http://localhost/?{qs}")).ok()
        });
        let url = url.ok_or_else(|| Error::Auth("could not parse the pasted URL".into()))?;
        let (code, got_state, _) = params(&url);
        if got_state.as_deref() != Some(state) {
            return Err(Error::Auth(
                "the pasted URL's state does not match this login session".into(),
            ));
        }
        return code.map(Some).ok_or_else(|| Error::Auth("no code in the pasted URL".into()));
    }
    Ok(Some(line.to_string()))
}

/// Pull `(code, state, error)` out of a callback URL's query.
fn params(url: &Url) -> (Option<String>, Option<String>, Option<String>) {
    let (mut code, mut state, mut err) = (None, None, None);
    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "code" => code = Some(v.into_owned()),
            "state" => state = Some(v.into_owned()),
            "error" => err = Some(v.into_owned()),
            _ => {}
        }
    }
    (code, state, err)
}

/// Read the HTTP request line and return its target (`/auth/callback?...`).
async fn request_target(sock: &mut TcpStream) -> Option<String> {
    let mut buf = [0u8; 8192];
    // Bound the read: a silent / half-open connection (browser preconnect probe,
    // port scanner, …) must not wedge the accept loop and starve the stdin paste
    // fallback. A stall is treated as an unrelated request and dropped.
    let n = timeout(Duration::from_secs(5), sock.read(&mut buf)).await.ok()?.ok()?;
    let text = String::from_utf8_lossy(&buf[..n]);
    // First line: `GET /auth/callback?code=... HTTP/1.1`.
    text.lines().next()?.split_whitespace().nth(1).map(str::to_owned)
}

async fn respond(sock: &mut TcpStream, status: &str, body: &str) -> Result<()> {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    sock.write_all(response.as_bytes())
        .await
        .map_err(|e| Error::Auth(format!("callback response failed: {e}")))
}

fn page(message: &str) -> String {
    format!("<!doctype html><html><body style=\"font-family:system-ui;padding:3rem\"><h2>Albert</h2><p>{message}</p></body></html>")
}

/// Exchange the authorization code for tokens (form-encoded, `authorization_code`).
async fn exchange_code(code: &str, redirect_uri: &str, verifier: &str) -> Result<TokenResponse> {
    let resp = HttpClient::new()
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", CLIENT_ID),
            ("code_verifier", verifier),
        ])
        .send()
        .await
        .map_err(|e| Error::Auth(format!("code exchange request failed: {e}")))?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(Error::Auth(format!("code exchange rejected ({status}): {body}")));
    }
    from_str(&body)
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: String,
    #[serde(default)]
    id_token: String,
}

fn from_str(body: &str) -> Result<TokenResponse> {
    serde_json::from_str(body).map_err(|e| Error::Auth(format!("token response parse: {e}")))
}

/// Random URL-safe base64 (no padding) from `n` OS-random bytes — used for the PKCE
/// verifier and the anti-CSRF state.
fn random_b64(n: usize) -> String {
    let mut buf = vec![0u8; n];
    OsRng.fill_bytes(&mut buf);
    URL_SAFE_NO_PAD.encode(buf)
}

/// Best-effort: open the system browser at `url`. A failure just means the user
/// opens the printed URL themselves.
fn open_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = Command::new("open");
        c.arg(url);
        c
    };
    #[cfg(target_os = "linux")]
    let mut cmd = {
        let mut c = Command::new("xdg-open");
        c.arg(url);
        c
    };
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = Command::new("cmd");
        c.args(["/C", "start", "", url]);
        c
    };
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    let mut cmd = Command::new("true");

    cmd.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::parse_pasted;

    #[test]
    fn full_url_with_matching_state_yields_code() {
        let line = "http://localhost:1455/auth/callback?code=abc123&state=S1&scope=openid";
        assert_eq!(parse_pasted(line, "S1").unwrap().as_deref(), Some("abc123"));
    }

    #[test]
    fn bare_query_string_is_accepted() {
        // No scheme/host — the localhost fallback still parses the query.
        let line = "code=xyz&state=S2";
        assert_eq!(parse_pasted(line, "S2").unwrap().as_deref(), Some("xyz"));
    }

    #[test]
    fn url_with_wrong_state_is_rejected() {
        let line = "http://localhost:1455/auth/callback?code=abc&state=EVIL";
        assert!(parse_pasted(line, "S1").is_err());
    }

    #[test]
    fn bare_code_is_taken_verbatim() {
        // No `code=`, so the whole line is the code (PKCE binds it to the session).
        assert_eq!(parse_pasted("just-the-code", "S1").unwrap().as_deref(), Some("just-the-code"));
    }

    #[test]
    fn empty_line_is_ignored() {
        assert_eq!(parse_pasted("", "S1").unwrap(), None);
    }
}
