//! Connection setup for the dedicated memory session.

use std::{env::var, time::Duration};

use mcp_http::{redirect::Policy, Client};
use rmcp::{
    model::{ClientInfo, Implementation},
    transport::{
        streamable_http_client::StreamableHttpClientTransportConfig, StreamableHttpClientTransport,
        TokioChildProcess,
    },
    ServiceExt,
};
use tokio::{process::Command, time::timeout};

use super::{McpTransport, MemoryError, Session};

pub(super) fn client_info() -> ClientInfo {
    let mut info = ClientInfo::default();
    info.client_info = Implementation::new("albert-memory", env!("CARGO_PKG_VERSION"));
    info
}

pub(super) async fn connect(
    transport: &McpTransport,
    duration: Duration,
) -> Result<Session, MemoryError> {
    timeout(duration, async {
        match transport {
            McpTransport::Http { url, token_env } => {
                let mut config = StreamableHttpClientTransportConfig::with_uri(url.clone())
                    .reinit_on_expired_session(false);
                if let Some(env) = token_env {
                    config = config.auth_header(secret(env)?);
                }
                let client = Client::builder()
                    .redirect(Policy::none())
                    .connect_timeout(duration)
                    .build()
                    .map_err(|e| MemoryError::Connection(e.to_string()))?;
                client_info()
                    .serve(StreamableHttpClientTransport::with_client(client, config))
                    .await
                    .map_err(|e| MemoryError::Connection(e.to_string()))
            }
            McpTransport::Stdio { command, args, env } => {
                let mut child = Command::new(command);
                child.args(args).env_clear();
                // Do not hand the server Albert's LLM, Telegram or cloud credentials.
                for name in [
                    "PATH",
                    "HOME",
                    "LANG",
                    "TMPDIR",
                    "XDG_CONFIG_HOME",
                    "XDG_DATA_HOME",
                ] {
                    if let Ok(value) = var(name) {
                        child.env(name, value);
                    }
                }
                for (name, source) in env {
                    child.env(name, secret(source)?);
                }
                let transport = TokioChildProcess::new(child)
                    .map_err(|e| MemoryError::Connection(e.to_string()))?;
                client_info()
                    .serve(transport)
                    .await
                    .map_err(|e| MemoryError::Connection(e.to_string()))
            }
        }
    })
    .await
    .map_err(|_| MemoryError::Timeout)?
}

fn secret(name: &str) -> Result<String, MemoryError> {
    var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| MemoryError::Config(format!("missing or empty environment variable {name}")))
}
