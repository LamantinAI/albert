use super::{fixture::fixture, AlbertCogitator};
use crate::{
    config::AuthMode,
    models::{ModelPool, ModelSpec, PoolConfig},
};
use axum::{extract::State, http::StatusCode, routing::post, serve, Json, Router};
use octo_core::CogitatorContext;
use serde_json::{json, Value};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{net::TcpListener, spawn, task::JoinHandle};
pub(super) struct Server {
    pub(super) requests: Arc<Mutex<Vec<Value>>>,
    task: JoinHandle<()>,
    url: String,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn completion(
    State(requests): State<Arc<Mutex<Vec<Value>>>>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let model = body["model"].as_str().unwrap();
    let count = {
        let mut requests = requests.lock().unwrap();
        requests.push(body.clone());
        requests.iter().filter(|r| r["model"] == model).count()
    };
    if model == "hanging" {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    if model == "failing" || (model == "writes" && count > 1) {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":{"code":503,"message":"overloaded"}})),
        );
    }
    if model == "incompatible" {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":{"code":400,"message":"This model does not support images"}})),
        );
    }
    let (message, finish) = if matches!(model, "file-dispatch" | "file-native") && count == 1 {
        let (name, args) = if model == "file-dispatch" {
            (
                "dispatch_to_connector",
                json!({"target":"telegram","kind":"chat.send_file","payload":{"path":"photo.jpg"}}),
            )
        } else {
            ("send_file", json!({"path":"photo.jpg"}))
        };
        (
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"file-call","type":"function","function":{"name":name,"arguments":args.to_string()}}]}),
            "tool_calls",
        )
    } else if model == "switches" && count == 1 {
        (
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"call_select","type":"function","function":{"name":"model_select","arguments":"{\"model_id\":\"healthy\"}"}}]}),
            "tool_calls",
        )
    } else if model == "writes" {
        (
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"call_note","type":"function","function":{"name":"scratchpad_note","arguments":"{\"text\":\"recorded once\"}"}}]}),
            "tool_calls",
        )
    } else {
        (
            json!({"role":"assistant","content":if model == "empty" { " ".to_string() } else { format!("answer from {model}") }}),
            "stop",
        )
    };
    (
        StatusCode::OK,
        Json(
            json!({"id":"test-completion","object":"chat.completion","created":1,"model":model,
            "choices":[{"index":0,"message":message,"finish_reason":finish}],
            "usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}),
        ),
    )
}

pub(super) async fn setup(first: &str) -> (Arc<AlbertCogitator>, CogitatorContext, Server) {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/chat/completions", post(completion))
        .with_state(requests.clone());
    let task = spawn(async move {
        serve(listener, app).await.unwrap();
    });
    let server = Server {
        requests,
        task,
        url,
    };
    let (mut agent, ctx, _) = fixture();
    let me = Arc::get_mut(&mut agent).unwrap();
    me.config.api_key = Some("local-test-key".into());
    me.config.models = Some(PoolConfig {
        default: first.into(),
        max_attempts: 3,
        retries_per_model: 0,
        retry_delay_ms: 0,
        models: [first, "healthy"]
            .into_iter()
            .map(|id| ModelSpec {
                id: id.into(),
                model: id.into(),
                provider: AuthMode::ApiKey,
                base_url: Some(server.url.clone()),
                api_key_env: None,
                vision: true,
                tools: true,
                request_timeout_ms: if id == "hanging" { 20 } else { 2000 },
            })
            .collect(),
    });
    me.models = ModelPool::new(&me.config);
    (agent, ctx, server)
}
