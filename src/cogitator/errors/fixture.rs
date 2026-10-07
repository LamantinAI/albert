use std::{
    convert::Infallible,
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
use bytes::Bytes;
use futures::{
    stream::{once, pending},
    StreamExt,
};
use rig::{completion::CompletionModel, http_client::ReqwestClient, providers::openai};
use serde_json::{json, Value};
use tokio::{net::TcpListener, spawn, task::JoinHandle};

use crate::{codex_http::CodexHttp, codex_model::CodexResponsesModel};

pub(super) struct Server {
    pub requests: Arc<Mutex<Vec<String>>>,
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
        let app = Router::new()
            .route("/responses", post(respond))
            .with_state(requests.clone());
        let task = spawn(async move { serve(listener, app).await.unwrap() });
        Self {
            requests,
            url,
            task,
        }
    }
    pub fn model(&self, id: &str) -> CodexResponsesModel {
        let http = ReqwestClient::builder()
            .timeout(Duration::from_millis(100))
            .build()
            .unwrap();
        let client = openai::Client::builder()
            .api_key("local-test-key")
            .base_url(&self.url)
            .http_client(CodexHttp::with_client(http))
            .build()
            .unwrap();
        CodexResponsesModel::make(&client, id)
    }
}
async fn respond(
    State(requests): State<Arc<Mutex<Vec<String>>>>,
    Json(body): Json<Value>,
) -> Response {
    let model = body["model"].as_str().unwrap().to_owned();
    let count = {
        let mut seen = requests.lock().unwrap();
        seen.push(model.clone());
        seen.iter().filter(|id| *id == &model).count()
    };
    if model == "unauthorized" {
        return Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .body(Body::from("expired token"))
            .unwrap();
    }
    if model == "bad-request" {
        return Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Body::from("invalid request"))
            .unwrap();
    }
    let delta = json!({"type":"response.output_text.delta","item_id":"msg_1","output_index":0,"content_index":0,"delta":"summary"});
    let mut frames = format!("data: {delta}\n\n");
    if model == "healthy" || model == "keep-open" || (model == "stall-once" && count > 1) {
        let complete = json!({"type":"response.completed","sequence_number":2,"response":{
            "id":"resp_1","object":"response","created_at":0,"status":"completed","model":model,
            "output":[],"tools":[],"usage":{"input_tokens":10,"output_tokens":2,"total_tokens":12}}});
        frames += &format!("data: {complete}\n\n");
    }
    let body = if model == "stall" || model == "keep-open" || (model == "stall-once" && count == 1)
    {
        Body::from_stream(
            once(async move { Ok::<Bytes, Infallible>(Bytes::from(frames)) }).chain(pending()),
        )
    } else {
        Body::from(frames)
    };
    Response::builder()
        .header(CONTENT_TYPE, "text/event-stream")
        .body(body)
        .unwrap()
}
