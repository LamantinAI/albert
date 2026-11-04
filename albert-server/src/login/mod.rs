//! Interactive loopback/stdin adapter for the shared subscription OAuth core.
use std::{io::Result as IoResult, path::Path, process::Command, process::Stdio, time::Duration};

use albert_core::{
    error::Error,
    error::Result,
    openai_login::{LoginFlow, OAuthEndpoints},
};
use octo_openai_auth::{plan_from_jwt, SubscriptionAuth};
use tokio::{
    io::stdin, io::AsyncBufReadExt, io::AsyncReadExt, io::AsyncWriteExt, io::BufReader,
    net::TcpListener, net::TcpStream, select, time::timeout,
};
use tracing::warn;

const PORTS: [u16; 2] = [1455, 1457];

pub async fn run(store_path: &Path) -> Result<()> {
    let (listener, port) = bind_callback().await?;
    let endpoints = OAuthEndpoints::startup()?;
    let flow = LoginFlow::for_redirect(
        &endpoints,
        &format!("http://localhost:{port}/auth/callback"),
    );
    println!("\nSign in with your ChatGPT account — open this URL in your browser:\n\n  {}\n\nWaiting for the redirect ...\n", flow.authorize_url());
    if let Err(error) = open_browser(flow.authorize_url()) {
        warn!(%error, "could not open a browser automatically; open the URL above manually");
    }
    let code = await_code(&listener, &flow).await?;
    let tokens = flow.exchange(&code).await?;
    let plan = plan_from_jwt(&tokens.access_token);
    SubscriptionAuth::new(store_path.to_owned())
        .install(tokens)
        .await?;
    println!(
        "Signed in{}. Tokens saved.",
        plan.map(|p| format!(" (plan: {p})")).unwrap_or_default()
    );
    Ok(())
}

async fn bind_callback() -> Result<(TcpListener, u16)> {
    for port in PORTS {
        if let Ok(listener) = TcpListener::bind(("127.0.0.1", port)).await {
            return Ok((listener, port));
        }
    }
    Err(Error::Auth("could not bind a login callback port".into()))
}

/// Wait for the authorization `code` two ways at once, whichever lands first:
///
/// - the browser redirect hits the loopback callback server (works when the browser
///   and Albert share a machine), or
/// - the user pastes the redirect URL (or the bare code) on stdin — the headless /
///   remote-VM fallback, since a browser elsewhere can't reach this box's loopback.
async fn await_code(listener: &TcpListener, flow: &LoginFlow) -> Result<String> {
    println!("...or paste the redirect URL (or just the code) here and press Enter.\n");
    let mut lines = BufReader::new(stdin()).lines();
    let mut stdin_open = true;
    loop {
        select! {
            accepted = listener.accept() => {
                let (mut sock, _) = accepted
                    .map_err(|e| Error::Auth(format!("callback accept failed: {e}")))?;
                if let Some(code) = handle_callback(&mut sock, flow).await? {
                    return Ok(code);
                }
            }
            line = lines.next_line(), if stdin_open => match line {
                Ok(Some(l)) => {
                    if let Some(code) = flow.parse_submission(l.trim())? {
                        return Ok(code);
                    }
                }
                Ok(None) => stdin_open = false, // keep waiting on the socket
                Err(e) => return Err(Error::Auth(format!("stdin read failed: {e}"))),
            },
        }
    }
}

async fn handle_callback(sock: &mut TcpStream, flow: &LoginFlow) -> Result<Option<String>> {
    let Some(target) = request_target(sock).await else {
        return Ok(None);
    };
    if !target.starts_with("/auth/callback?") {
        let _ = respond(sock, "404 Not Found", "").await;
        return Ok(None);
    }
    let input = format!("{}{}", flow.callback_origin(), target);
    match flow.parse_submission(&input) {
        Ok(Some(code)) => {
            let _ = respond(
                sock,
                "200 OK",
                &page("Authorization received. Return to Albert."),
            )
            .await;
            Ok(Some(code))
        }
        _ => {
            let _ = respond(
                sock,
                "400 Bad Request",
                &page("Unrecognized or invalid callback. Return to Albert to retry."),
            )
            .await;
            Ok(None)
        }
    }
}

/// Read the HTTP request line and return its target (`/auth/callback?...`).
async fn request_target(sock: &mut TcpStream) -> Option<String> {
    let mut buf = [0u8; 8192];
    // Bound the read: a silent / half-open connection (browser preconnect probe,
    // port scanner, …) must not wedge the accept loop and starve the stdin paste
    // fallback. A stall is treated as an unrelated request and dropped.
    let n = timeout(Duration::from_secs(5), sock.read(&mut buf))
        .await
        .ok()?
        .ok()?;
    let text = String::from_utf8_lossy(&buf[..n]);
    // First line: `GET /auth/callback?code=... HTTP/1.1`.
    let mut request = text.lines().next()?.split_whitespace();
    if request.next()? != "GET" {
        return None;
    }
    request.next().map(str::to_owned)
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

/// Best-effort: open the system browser at `url`. A failure just means the user
/// opens the printed URL themselves.
fn open_browser(url: &str) -> IoResult<()> {
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

    cmd.stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::{
        io::AsyncReadExt, io::AsyncWriteExt, net::TcpListener, net::TcpStream, spawn, time::timeout,
    };
    use url::Url;

    use super::{handle_callback, LoginFlow, OAuthEndpoints};

    #[tokio::test]
    async fn loopback_accepts_only_matching_get_callback_and_ignores_strays() {
        timeout(Duration::from_secs(3), async {
            for valid in [false, true] {
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let address = listener.local_addr().unwrap();
                let flow = LoginFlow::new(&OAuthEndpoints::startup().unwrap());
                let url = Url::parse(flow.authorize_url()).unwrap();
                let state = url.query_pairs().find(|(key, _)| key == "state").unwrap().1.into_owned();
                let client = spawn(async move {
                    let mut socket = TcpStream::connect(address).await.unwrap();
                    let request = format!("GET /auth/callback?code=synthetic-code&state={} HTTP/1.1\r\nHost: localhost\r\n\r\n", if valid { state.as_str() } else { "wrong-state" });
                    socket.write_all(request.as_bytes()).await.unwrap();
                    let mut response = [0u8; 1024];
                    let _ = socket.read(&mut response).await.unwrap();
                });
                let (mut socket, _) = listener.accept().await.unwrap();
                let code = handle_callback(&mut socket, &flow).await.unwrap();
                assert_eq!(code.is_some(), valid);
                client.await.unwrap();
            }
        }).await.expect("bounded loopback callback test");
    }
}
