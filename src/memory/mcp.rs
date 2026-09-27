//! A long-lived MCP session for memory. No silent fallback to an empty local vault.

use std::{
    collections::{HashMap, HashSet},
    env::var,
    sync::Arc,
    time::Duration,
};

use mcp_http::{redirect::Policy, Client};
use rig::tool::ToolDyn;
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, ClientInfo, Implementation, JsonObject, Tool},
    service::{RoleClient, RunningService},
    transport::{
        streamable_http_client::StreamableHttpClientTransportConfig, StreamableHttpClientTransport,
        TokioChildProcess,
    },
    ServiceExt,
};
use serde_json::Value;
use tokio::{process::Command, sync::Mutex, time::timeout};
use tracing::info;

use super::{
    config::McpTransport,
    migration::{has_initiative, seed},
    tool::MemoryTool,
    MemoryError, INITIATIVE,
};

pub type Session = RunningService<RoleClient, ClientInfo>;

pub struct McpMemory {
    transport: McpTransport,
    timeout: Duration,
    definitions: Vec<Tool>,
    session: Mutex<Option<Arc<Session>>>,
}

impl McpMemory {
    pub async fn open(
        transport: McpTransport,
        timeout_secs: u64,
    ) -> Result<Arc<Self>, MemoryError> {
        let duration = Duration::from_secs(timeout_secs);
        let session = connect(&transport, duration).await?;
        Self::from_session(transport, duration, session).await
    }

    async fn from_session(
        transport: McpTransport,
        duration: Duration,
        session: Session,
    ) -> Result<Arc<Self>, MemoryError> {
        let definitions = prepare(&session, duration).await?;
        Ok(Arc::new(Self {
            transport,
            timeout: duration,
            definitions,
            session: Mutex::new(Some(Arc::new(session))),
        }))
    }

    pub fn tools(self: &Arc<Self>) -> Vec<Box<dyn ToolDyn>> {
        self.definitions
            .iter()
            .map(|definition| {
                Box::new(MemoryTool::new(definition.clone(), self.clone())) as Box<dyn ToolDyn>
            })
            .collect()
    }

    async fn session(&self) -> Result<Arc<Session>, MemoryError> {
        let mut current = self.session.lock().await;
        if let Some(session) = current.as_ref().filter(|session| !session.is_closed()) {
            return Ok(session.clone());
        }
        let session = connect(&self.transport, self.timeout).await?;
        let definitions = prepare(&session, self.timeout).await?;
        // A restarted server may have changed its contract. Do not call it with
        // schemas already advertised to a running model; require an Albert restart.
        if signatures(&definitions) != signatures(&self.definitions) {
            return Err(MemoryError::Protocol(
                "memory tool schemas changed; restart Albert".into(),
            ));
        }
        let session = Arc::new(session);
        *current = Some(session.clone());
        Ok(session)
    }

    pub async fn call(
        &self,
        name: &str,
        arguments: JsonObject,
    ) -> Result<CallToolResult, MemoryError> {
        let session = self.session().await?;
        let result = call(&session, self.timeout, name, arguments).await;
        if matches!(
            result,
            Err(MemoryError::Connection(_) | MemoryError::Timeout)
        ) {
            // An interrupted write may already have committed. Never replay it.
            // Reconnect for the NEXT call, without disturbing a newer connection.
            let mut current = self.session.lock().await;
            if current
                .as_ref()
                .is_some_and(|active| Arc::ptr_eq(active, &session))
            {
                current.take();
            }
        }
        result
    }
}

fn signatures(tools: &[Tool]) -> HashMap<String, Value> {
    tools
        .iter()
        .map(|t| (t.name.to_string(), t.schema_as_json_value()))
        .collect()
}

fn client_info() -> ClientInfo {
    let mut info = ClientInfo::default();
    info.client_info = Implementation::new("albert-memory", env!("CARGO_PKG_VERSION"));
    info
}

async fn connect(transport: &McpTransport, duration: Duration) -> Result<Session, MemoryError> {
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

async fn prepare(session: &Session, duration: Duration) -> Result<Vec<Tool>, MemoryError> {
    let tools = timeout(duration, session.list_all_tools())
        .await
        .map_err(|_| MemoryError::Timeout)?
        .map_err(|e| MemoryError::Connection(e.to_string()))?;
    let mut names = HashSet::new();
    for tool in &tools {
        let name = tool.name.as_ref();
        if !names.insert(name)
            || name.is_empty()
            || name.len() > 58
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
            || name.starts_with("kaeru_")
        {
            return Err(MemoryError::Protocol(
                "invalid or duplicate memory tool name".into(),
            ));
        }
    }
    // These verbs define the Kaeru memory contract and are used by Albert's
    // standing instructions and automatic reflection routine.
    for required in [
        "initiatives",
        "cite",
        "awake",
        "overview",
        "episode",
        "at",
        "reflect",
    ] {
        if !names.contains(required) {
            return Err(MemoryError::Protocol(format!(
                "memory server is missing {required}"
            )));
        }
    }
    let existing = call(session, duration, "initiatives", JsonObject::new()).await?;
    if !has_initiative(&existing)? {
        call(
            session,
            duration,
            "cite",
            seed().as_object().expect("object literal").clone(),
        )
        .await?;
        let verified = call(session, duration, "initiatives", JsonObject::new()).await?;
        if !has_initiative(&verified)? {
            return Err(MemoryError::Protocol(
                "migration did not create the albert initiative".into(),
            ));
        }
        info!(
            initiative = INITIATIVE,
            migration = 1,
            "memory migration applied"
        );
    }
    Ok(tools)
}

async fn call(
    session: &Session,
    duration: Duration,
    name: &str,
    arguments: JsonObject,
) -> Result<CallToolResult, MemoryError> {
    let request = CallToolRequestParams::new(name.to_string()).with_arguments(arguments);
    let result = timeout(duration, session.call_tool(request))
        .await
        .map_err(|_| MemoryError::Timeout)?
        .map_err(|e| MemoryError::Connection(e.to_string()))?;
    if result.is_error == Some(true) {
        let text = result
            .content
            .iter()
            .filter_map(|c| c.raw.as_text())
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        return Err(MemoryError::Tool(text));
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
