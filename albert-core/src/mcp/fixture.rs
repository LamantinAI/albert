use axum::{serve, Router};
use rmcp::{
    model::CallToolRequestParams, model::CallToolResult, model::ListToolsResult,
    model::PaginatedRequestParams, model::ServerCapabilities, model::ServerInfo, model::Tool,
    service::RequestContext, service::RoleServer,
    transport::streamable_http_server::session::local::LocalSessionManager,
    transport::streamable_http_server::StreamableHttpServerConfig,
    transport::streamable_http_server::StreamableHttpService, ErrorData, ServerHandler,
};
use serde_json::json;
use std::{sync::Arc, sync::Mutex};
use tokio::{net::TcpListener, spawn, task::JoinHandle};

#[derive(Default)]
pub struct State {
    pub version: usize,
    pub calls: Vec<String>,
}
#[derive(Clone, Default)]
pub struct Fake(pub Arc<Mutex<State>>);
impl ServerHandler for Fake {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
    }
    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let version = self.0.lock().unwrap().version;
        Ok(ListToolsResult {
            tools: ["read", "write"]
                .into_iter()
                .map(|name| {
                    Tool::new(
                        name,
                        "local native MCP fixture",
                        json!({"type":"object","properties":{"version":{"const":version}}})
                            .as_object()
                            .unwrap()
                            .clone(),
                    )
                })
                .collect(),
            ..Default::default()
        })
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        self.0.lock().unwrap().calls.push(request.name.to_string());
        Ok(CallToolResult::structured(
            json!({"tool":request.name,"arguments":request.arguments.unwrap_or_default()}),
        ))
    }
}
pub async fn server(fake: Fake) -> (String, JoinHandle<()>) {
    let mut config = StreamableHttpServerConfig::default();
    config.stateful_mode = false;
    config.json_response = true;
    let service = StreamableHttpService::new(
        move || Ok(fake.clone()),
        Arc::new(LocalSessionManager::default()),
        config,
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let task = spawn(async move {
        serve(listener, Router::new().nest_service("/mcp", service))
            .await
            .unwrap();
    });
    (url, task)
}
