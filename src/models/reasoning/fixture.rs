use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    body::Body,
    extract::State,
    http::{header::CONTENT_TYPE, StatusCode},
    response::Response,
    routing::post,
    serve, Json, Router,
};
use rig::{
    client::CompletionClient,
    completion::CompletionModel,
    http_client::ReqwestClient,
    providers::{openai, openrouter::Client as OpenRouterClient},
};
use serde_json::{json, Value};
use tokio::{net::TcpListener, spawn, task::JoinHandle};

use crate::{
    codex_http::CodexHttp, codex_model::CodexResponsesModel, openrouter_http::OpenRouterHttp,
};

pub(super) struct Server {
    pub requests: Arc<Mutex<Vec<Value>>>,
    url: String,
    task: JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Server {
    pub async fn new() -> Self {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let router = Router::new()
            .route("/chat/completions", post(chat))
            .route("/responses", post(responses))
            .with_state(requests.clone());
        let task = spawn(async move { serve(listener, router).await.unwrap() });
        Self {
            requests,
            url,
            task,
        }
    }
    pub fn api(&self) -> impl CompletionModel + 'static {
        let http = ReqwestClient::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        OpenRouterClient::builder()
            .api_key("fixture-key")
            .base_url(&self.url)
            .http_client(OpenRouterHttp::new(http))
            .build()
            .unwrap()
            .completion_model("fixture-model")
    }
    pub fn subscription(&self) -> CodexResponsesModel {
        let http = ReqwestClient::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        let client = openai::Client::builder()
            .api_key("fixture-key")
            .base_url(&self.url)
            .http_client(CodexHttp::with_client(http))
            .build()
            .unwrap();
        CodexResponsesModel::make(&client, "fixture-model")
    }
}
async fn chat(State(requests): State<Arc<Mutex<Vec<Value>>>>, Json(body): Json<Value>) -> Response {
    let count = {
        let mut requests = requests.lock().unwrap();
        requests.push(body.clone());
        requests.len()
    };
    if body["retry_fixture"] == true && count == 1 {
        return Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({"error":{"code":503,"message":"retry"}}).to_string(),
            ))
            .unwrap();
    }
    Response::builder().header(CONTENT_TYPE,"application/json").body(Body::from(json!({"id":"fixture","object":"chat.completion","created":1,"model":"fixture-model","choices":[{"index":0,"message":{"role":"assistant","content":"answer"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}).to_string())).unwrap()
}
async fn responses(
    State(requests): State<Arc<Mutex<Vec<Value>>>>,
    Json(body): Json<Value>,
) -> Response {
    requests.lock().unwrap().push(body);
    let delta = json!({"type":"response.output_text.delta","item_id":"msg_1","output_index":0,"content_index":0,"delta":"answer"});
    let complete = json!({"type":"response.completed","sequence_number":2,"response":{"id":"resp_1","object":"response","created_at":0,"status":"completed","model":"fixture-model","output":[],"tools":[],"usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}});
    Response::builder()
        .header(CONTENT_TYPE, "text/event-stream")
        .body(Body::from(format!("data: {delta}\n\ndata: {complete}\n\n")))
        .unwrap()
}
