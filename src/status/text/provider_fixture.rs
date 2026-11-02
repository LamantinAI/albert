use std::{convert::Infallible, sync::Arc};

use axum::{body::Body, extract::State, routing::post, serve, Json, Router};
use bytes::Bytes;
use futures::stream::unfold;
use serde_json::{json, Value};
use tokio::{net::TcpListener, spawn, sync::Notify, task::JoinHandle};

pub(super) struct Server {
    pub url: String,
    pub gate: Arc<Notify>,
    task: JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Server {
    pub async fn api() -> Self {
        let gate = Arc::new(Notify::new());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let router = Router::new()
            .route("/chat/completions", post(api_response))
            .with_state(gate.clone());
        let task = spawn(async move { serve(listener, router).await.unwrap() });
        Self { url, gate, task }
    }
    pub async fn start(truncated: bool) -> Self {
        let gate = Arc::new(Notify::new());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let router = Router::new()
            .route("/responses", post(response))
            .with_state((gate.clone(), truncated));
        let task = spawn(async move { serve(listener, router).await.unwrap() });
        Self { url, gate, task }
    }
}
async fn api_response(
    State(gate): State<Arc<Notify>>,
    Json(body): Json<Value>,
) -> ([(&'static str, &'static str); 1], Body) {
    assert_eq!(body["stream"], true);
    assert_eq!(body["stream_options"]["include_usage"], true);
    let prefix = json!({"id":"response-id","model":"fixture","choices":[{"index":0,"delta":{"content":"API text"},"finish_reason":null}]});
    let finish = json!({"id":"response-id","model":"fixture","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]});
    let usage = json!({"id":"response-id","model":"fixture","choices":[],"usage":{"prompt_tokens":31,"completion_tokens":11,"total_tokens":42}});
    let body = Body::from_stream(unfold(
        (0, gate, prefix, finish, usage),
        |(stage, gate, prefix, finish, usage)| async move {
            let chunk = match stage {
                0 => format!("data: {prefix}\n\n"),
                1 => {
                    gate.notified().await;
                    format!("data: {finish}\n\ndata: {usage}\n\ndata: [DONE]\n\n")
                }
                _ => return None,
            };
            Some((
                Ok::<_, Infallible>(Bytes::from(chunk)),
                (stage + 1, gate, prefix, finish, usage),
            ))
        },
    ));
    ([("content-type", "text/event-stream")], body)
}
async fn response(
    State((gate, truncated)): State<(Arc<Notify>, bool)>,
    Json(body): Json<Value>,
) -> ([(&'static str, &'static str); 1], Body) {
    assert_eq!(body["stream"], true);
    assert_eq!(body["store"], false);
    let text = json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"sequence_number":1,"delta":"Visible text"});
    let reasoning = json!({"type":"response.output_item.done","output_index":1,"sequence_number":2,"item":{"type":"reasoning","id":"reasoning-item","summary":[{"type":"summary_text","text":"private-summary-canary"}],"encrypted_content":"opaque-signature-canary","status":"completed"}});
    let tool = json!({"type":"response.output_item.done","output_index":2,"sequence_number":3,"item":{"type":"function_call","id":"tool-item","call_id":"provider-call","name":"sample_tool","arguments":"{\"value\":7}","status":"completed"}});
    let message = json!({"type":"response.output_item.done","output_index":0,"sequence_number":4,"item":{"type":"message","id":"message-item","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Visible text","annotations":[]}]}});
    let terminal = json!({"type":"response.completed","sequence_number":5,"response":{"id":"response-id","object":"response","created_at":0,"status":"completed","model":"fixture","output":[],"usage":{"input_tokens":31,"output_tokens":11,"total_tokens":42,"input_tokens_details":{"cached_tokens":5},"output_tokens_details":{"reasoning_tokens":3}}}});
    let prefix =
        format!("data: {text}\n\ndata: {reasoning}\n\ndata: {tool}\n\ndata: {message}\n\n");
    let body = Body::from_stream(unfold(
        (0, gate, prefix, terminal),
        move |(stage, gate, prefix, terminal)| async move {
            let chunk = match stage {
                0 => prefix.clone(),
                1 => {
                    gate.notified().await;
                    if truncated {
                        return None;
                    }
                    format!("data: {terminal}\n\n")
                }
                _ => return None,
            };
            Some((
                Ok::<_, Infallible>(Bytes::from(chunk)),
                (stage + 1, gate, prefix, terminal),
            ))
        },
    ));
    ([("content-type", "text/event-stream")], body)
}
