use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};

pub(super) fn response(status: StatusCode, body: Value, streaming: bool) -> Response {
    if !streaming || !status.is_success() {
        return (status, Json(body)).into_response();
    }
    let mut delta = body["choices"][0]["message"].clone();
    if let Some(calls) = delta.get_mut("tool_calls").and_then(Value::as_array_mut) {
        for (index, call) in calls.iter_mut().enumerate() {
            call["index"] = json!(index);
        }
    }
    let first = json!({"id":body["id"],"object":"chat.completion.chunk","created":0,"model":body["model"],"choices":[{"index":0,"delta":delta,"finish_reason":null}]});
    let final_frame = json!({"id":body["id"],"object":"chat.completion.chunk","created":0,"model":body["model"],"choices":[{"index":0,"delta":{},"finish_reason":body["choices"][0]["finish_reason"]}],"usage":body["usage"]});
    (
        status,
        [("content-type", "text/event-stream")],
        format!("data: {first}\n\ndata: {final_frame}\n\ndata: [DONE]\n\n"),
    )
        .into_response()
}
