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

use super::{client_info, McpMemory};
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
                "initiatives" => CallToolResult::success(vec![Content::text(if state.exists {
                    "initiatives (2):\n  - albert\n  - other-project\n"
                } else {
                    "initiatives (1):\n  - other-project\n"
                })]),
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
