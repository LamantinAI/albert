//! A long-lived MCP session for memory. No silent fallback to an empty local vault.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use rig::tool::ToolDyn;
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, ClientInfo, JsonObject, Tool},
    service::{RoleClient, RunningService},
};
use serde_json::Value;
use tokio::{sync::Mutex, time::timeout};
use tracing::info;

use super::{
    config::McpTransport,
    migration::{has_initiative, seed},
    tool::MemoryTool,
    MemoryError, INITIATIVE,
};

mod transport;
use self::transport::connect;

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
mod tests {
    //! Exercise the real MCP handshake, pagination and tool protocol against a
    //! disposable server. No production vault, credentials or model calls.

    use std::{
        env::{remove_var, set_var},
        sync::{Arc, Mutex},
        time::Duration,
    };

    use axum::{
        extract::Request,
        http::StatusCode,
        middleware::{from_fn, Next},
        response::Response,
        serve, Router,
    };
    use rig::tool::ToolDyn;
    use rmcp::{
        model::{
            CallToolRequestParams, CallToolResult, Content, JsonObject, ListToolsResult,
            PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool,
        },
        service::{RequestContext, RoleServer},
        transport::streamable_http_server::{
            session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
        },
        ErrorData, ServerHandler, ServiceExt,
    };
    use serde_json::{from_str, json, Value};
    use tokio::{io::duplex, net::TcpListener, spawn, time::sleep};

    use super::{transport::client_info, McpMemory};
    use crate::memory::{config::McpTransport, migration::NAME, MemoryError};

    #[derive(Default)]
    struct State {
        exists: bool,
        creates: usize,
        calls: Vec<(String, JsonObject)>,
        fail_cite: bool,
        invalid_list: bool,
        omit_reflect: bool,
        delay_write: bool,
        prefixed: bool,
    }

    #[derive(Clone, Default)]
    struct FakeMemory(Arc<Mutex<State>>);

    impl ServerHandler for FakeMemory {
        fn get_info(&self) -> ServerInfo {
            ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
        }

        async fn list_tools(
            &self,
            request: Option<PaginatedRequestParams>,
            _: RequestContext<RoleServer>,
        ) -> Result<ListToolsResult, ErrorData> {
            let state = self.0.lock().unwrap();
            let names = if request.and_then(|r| r.cursor).is_none() {
                vec!["initiatives", "cite", "awake", "overview"]
            } else if state.omit_reflect {
                vec!["episode", "at", "fail"]
            } else {
                vec!["episode", "at", "reflect", "fail"]
            };
            let first = names[0] == "initiatives";
            let tools = names
                .into_iter()
                .map(|name| {
                    let properties = if name == "initiatives" {
                        json!({})
                    } else {
                        json!({"initiative": {"type": ["string", "null"]}})
                    };
                    Tool::new(
                        if state.prefixed {
                            format!("kaeru_{name}")
                        } else {
                            name.into()
                        },
                        "test memory verb",
                        json!({"type": "object", "properties": properties})
                            .as_object()
                            .unwrap()
                            .clone(),
                    )
                })
                .collect();
            Ok(ListToolsResult {
                tools,
                next_cursor: first.then(|| "page2".into()),
                ..Default::default()
            })
        }

        async fn call_tool(
            &self,
            request: CallToolRequestParams,
            _: RequestContext<RoleServer>,
        ) -> Result<CallToolResult, ErrorData> {
            let name = request.name.to_string();
            let args = request.arguments.unwrap_or_default();
            let (result, delay) = {
                let mut state = self.0.lock().unwrap();
                state.calls.push((name.clone(), args.clone()));
                let result = match name.as_str() {
                    "initiatives" if state.invalid_list => {
                        CallToolResult::success(vec![Content::text("unexpected response")])
                    }
                    "initiatives" => {
                        CallToolResult::success(vec![Content::text(if state.exists {
                            "initiatives (2):\n  - albert\n  - other-project\n"
                        } else {
                            "initiatives (1):\n  - other-project\n"
                        })])
                    }
                    "cite" if state.fail_cite => {
                        CallToolResult::error(vec![Content::text("read-only vault")])
                    }
                    "cite" => {
                        assert_eq!(args["initiative"], "albert");
                        assert_eq!(args["name"], NAME);
                        state.exists = true;
                        state.creates += 1;
                        CallToolResult::success(vec![Content::text("created")])
                    }
                    "fail" => CallToolResult::error(vec![Content::text("memory denied")]),
                    _ => CallToolResult::structured(json!({"verb": name, "args": args})),
                };
                (result, state.delay_write && name == "episode")
            };
            if delay {
                sleep(Duration::from_millis(200)).await;
            }
            Ok(result)
        }
    }

    async fn attach(fake: FakeMemory, duration: Duration) -> Result<Arc<McpMemory>, MemoryError> {
        let (client, server) = duplex(65536);
        spawn(async move {
            let service = fake.serve(server).await.unwrap();
            let _ = service.waiting().await;
        });
        let session = client_info().serve(client).await.unwrap();
        McpMemory::from_session(
            McpTransport::Stdio {
                command: "/not-used".into(),
                args: vec![],
                env: Default::default(),
            },
            duration,
            session,
        )
        .await
    }

    fn verb(memory: &Arc<McpMemory>, name: &str) -> Box<dyn ToolDyn> {
        memory
            .tools()
            .into_iter()
            .find(|tool| tool.name() == name)
            .unwrap()
    }

    #[tokio::test]
    async fn first_connect_migrates_once_and_restart_preserves_existing_initiative() {
        let fake = FakeMemory::default();
        let first = attach(fake.clone(), Duration::from_secs(2)).await.unwrap();
        assert_eq!(fake.0.lock().unwrap().creates, 1);
        assert_eq!(first.tools().len(), 8, "tools/list pagination is followed");
        drop(first);
        let _second = attach(fake.clone(), Duration::from_secs(2)).await.unwrap();
        assert_eq!(fake.0.lock().unwrap().creates, 1);
    }

    #[tokio::test]
    async fn default_scope_explicit_scope_structured_results_and_errors_survive_adapter() {
        let fake = FakeMemory::default();
        let memory = attach(fake.clone(), Duration::from_secs(2)).await.unwrap();
        let episode = verb(&memory, "kaeru_episode");
        for (args, expected) in [
            ("{}", "albert"),
            ("{\"initiative\":null}", "albert"),
            ("{\"initiative\":\"research\"}", "research"),
        ] {
            let result: Value = from_str(&episode.call(args.into()).await.unwrap()).unwrap();
            assert_eq!(result["structuredContent"]["args"]["initiative"], expected);
        }
        assert!(episode.call("not json".into()).await.is_err());
        assert!(episode.call("[]".into()).await.is_err());
        let global = verb(&memory, "kaeru_initiatives");
        global.call("{}".into()).await.unwrap();
        assert!(fake.0.lock().unwrap().calls.last().unwrap().1.is_empty());
        let error = verb(&memory, "kaeru_fail")
            .call("{}".into())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("memory denied"));
        assert_eq!(
            episode.definition(String::new()).await.name,
            "kaeru_episode"
        );
    }

    #[tokio::test]
    async fn startup_failure_never_becomes_success_or_local_fallback() {
        for field in ["fail_cite", "invalid_list", "omit_reflect", "prefixed"] {
            let fake = FakeMemory::default();
            {
                let mut state = fake.0.lock().unwrap();
                match field {
                    "fail_cite" => state.fail_cite = true,
                    "invalid_list" => state.invalid_list = true,
                    "omit_reflect" => state.omit_reflect = true,
                    _ => state.prefixed = true,
                }
            }
            assert!(
                attach(fake.clone(), Duration::from_secs(2)).await.is_err(),
                "{field}"
            );
            assert_eq!(fake.0.lock().unwrap().creates, 0);
        }
    }

    #[tokio::test]
    async fn timed_out_write_is_not_replayed() {
        let fake = FakeMemory::default();
        let memory = attach(fake.clone(), Duration::from_millis(50))
            .await
            .unwrap();
        fake.0.lock().unwrap().delay_write = true;
        let error = verb(&memory, "kaeru_episode")
            .call("{}".into())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("timed out"));
        assert_eq!(
            fake.0
                .lock()
                .unwrap()
                .calls
                .iter()
                .filter(|(name, _)| name == "episode")
                .count(),
            1
        );
        assert!(memory.session.lock().await.is_none());
    }

    #[tokio::test]
    async fn streamable_http_connects_and_reconnects_without_reseeding() {
        let fake = FakeMemory::default();
        let factory = fake.clone();
        let service = StreamableHttpService::new(
            move || Ok(factory.clone()),
            Arc::new(LocalSessionManager::default()),
            StreamableHttpServerConfig::default(),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/mcp", listener.local_addr().unwrap());
        let server = spawn(async move {
            serve(listener, Router::new().nest_service("/mcp", service))
                .await
                .unwrap();
        });
        let memory = McpMemory::open(
            McpTransport::Http {
                url,
                token_env: None,
            },
            2,
        )
        .await
        .unwrap();
        verb(&memory, "kaeru_at").call("{}".into()).await.unwrap();
        memory.session.lock().await.take();
        verb(&memory, "kaeru_at").call("{}".into()).await.unwrap();
        assert_eq!(fake.0.lock().unwrap().creates, 1);
        assert_eq!(
            fake.0
                .lock()
                .unwrap()
                .calls
                .iter()
                .filter(|(name, _)| name == "at")
                .count(),
            2
        );
        // A changed server contract must not be used with the old model toolset.
        memory.session.lock().await.take();
        fake.0.lock().unwrap().omit_reflect = true;
        assert!(verb(&memory, "kaeru_at").call("{}".into()).await.is_err());
        drop(memory);
        server.abort();
    }

    async fn require_bearer(request: Request, next: Next) -> Result<Response, StatusCode> {
        if request
            .headers()
            .get("authorization")
            .and_then(|h| h.to_str().ok())
            != Some("Bearer memory-test-token")
        {
            return Err(StatusCode::UNAUTHORIZED);
        }
        Ok(next.run(request).await)
    }

    #[tokio::test]
    async fn http_json_transport_sends_bearer_and_missing_credentials_fail() {
        let fake = FakeMemory::default();
        let factory = fake.clone();
        let mut config = StreamableHttpServerConfig::default();
        config.stateful_mode = false;
        config.json_response = true;
        let service = StreamableHttpService::new(
            move || Ok(factory.clone()),
            Arc::new(LocalSessionManager::default()),
            config,
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/mcp", listener.local_addr().unwrap());
        let app = Router::new()
            .nest_service("/mcp", service)
            .layer(from_fn(require_bearer));
        let server = spawn(async move {
            serve(listener, app).await.unwrap();
        });
        let variable = "ALBERT_MCP_TEST_ONLY_BEARER";
        remove_var(variable);
        let transport = McpTransport::Http {
            url,
            token_env: Some(variable.into()),
        };
        assert!(matches!(
            McpMemory::open(transport.clone(), 2).await,
            Err(MemoryError::Config(_))
        ));
        set_var(variable, "memory-test-token");
        let result = McpMemory::open(transport, 2).await;
        remove_var(variable);
        let memory = result.unwrap();
        verb(&memory, "kaeru_at").call("{}".into()).await.unwrap();
        assert_eq!(fake.0.lock().unwrap().creates, 1);
        drop(memory);
        server.abort();
    }
}
