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
    if model.starts_with("never") {
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
    if model == "compact-delayed"
        && body["tools"]
            .as_array()
            .is_none_or(|tools| tools.is_empty())
    {
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    if model == "hanging" {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    if model == "failing" || (model == "writes" && count > 1) || (model == "recovers" && count == 2)
    {
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
    let (message, finish) = if model == "waiting-parent" && count == 1 {
        let wire = body["messages"].to_string();
        let rest = wire.split_once("WAIT_RUN:").unwrap().1.trim_start();
        let run_id: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || "/-_".contains(*c))
            .collect();
        (
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"wait-child","type":"function","function":{
                "name":"subagent","arguments":json!({"action":"wait","run_id":run_id,"seconds":60}).to_string()
            }}]}),
            "tool_calls",
        )
    } else if matches!(model, "budget-aware" | "budget-defiant")
        && (model == "budget-defiant"
            || body["tools"]
                .as_array()
                .is_some_and(|tools| !tools.is_empty()))
    {
        (
            json!({"role":"assistant","content":null,"tool_calls":[{"id":format!("budget-{count}"),"type":"function","function":{
                "name":"scratchpad_note","arguments":json!({"text":format!("evidence-{count}")}).to_string()
            }}]}),
            "tool_calls",
        )
    } else if model == "delegates" && count == 1 {
        (
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"spawn-child","type":"function","function":{
                "name":"subagent","arguments":json!({"action":"spawn","task":{"task":"Write a short answer","context":"Only this explicit context","models":["healthy"],"connectors":[],"tools":[]}}).to_string()
            }}]}),
            "tool_calls",
        )
    } else if matches!(model, "child-dispatch" | "large-output") && count == 1 {
        (
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"child-search","type":"function","function":{
                "name":"dispatch_to_connector","arguments":json!({"target":"search","kind":"search.web","payload":{"query":"test"}}).to_string()
            }}]}),
            "tool_calls",
        )
    } else if model == "large-output" && count == 2 {
        let result = body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .find(|m| m["role"] == "tool")
            .unwrap();
        let content: Value = serde_json::from_str(result["content"].as_str().unwrap()).unwrap();
        assert_eq!(content["offloaded"], true);
        assert!(result.to_string().len() < 14000);
        (
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"read-evidence","type":"function","function":{"name":"artifact","arguments":json!({"action":"read","id":content["artifact_id"],"field":["result","text"],"offset":20000,"limit":100}).to_string()}}]}),
            "tool_calls",
        )
    } else if model == "discovery" && count == 1 {
        (
            json!({"role":"assistant","content":null,"tool_calls":[{"id":"find-connector","type":"function","function":{"name":"connector_search","arguments":"{\"query\":\"inventory\"}"}}]}),
            "tool_calls",
        )
    } else if matches!(model, "file-dispatch" | "file-native") && count == 1 {
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
    } else if model == "writes" || (model == "recovers" && count == 1) {
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
                context_window: None,
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
