//! Preserve the gateway's original assistant tool rounds during ONE attempt.
//! Opaque reasoning_details/signatures are not reconstructed by model family,
//! persisted in the journal, or shared with a different model/client.
use std::{
    collections::HashMap,
    fmt::{Debug, Formatter, Result as FmtResult},
    future::Future,
    sync::{Arc, Mutex},
};

use bytes::Bytes;
use rig::{
    http_client::{
        Error as HttpError, HttpClientExt, LazyBody, MultipartForm, Request, ReqwestClient,
        Response, Result as HttpResult, StreamingResponse,
    },
    wasm_compat::WasmCompatSend,
};
use serde_json::{from_slice, to_vec, Value};

#[derive(Clone, Default)]
pub struct OpenRouterHttp {
    inner: ReqwestClient,
    rounds: Arc<Mutex<HashMap<Vec<String>, Vec<Value>>>>,
}

impl Debug for OpenRouterHttp {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.debug_struct("OpenRouterHttp").finish_non_exhaustive()
    }
}

fn call_ids(message: &Value) -> Option<Vec<String>> {
    let calls = message.get("tool_calls")?.as_array()?;
    if calls.is_empty() {
        return None;
    }
    calls
        .iter()
        .map(|call| call.get("id")?.as_str().map(str::to_owned))
        .collect()
}

impl OpenRouterHttp {
    pub fn new(inner: ReqwestClient) -> Self {
        Self {
            inner,
            rounds: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn restore<T: Into<Bytes>>(&self, request: Request<T>) -> Request<Bytes> {
        let (mut parts, body) = request.into_parts();
        let bytes = body.into();
        let bytes = if parts.uri.path().ends_with("/chat/completions")
            && !self.rounds.lock().unwrap().is_empty()
        {
            match from_slice::<Value>(&bytes) {
                Ok(mut body) => {
                    if let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) {
                        let rounds = self.rounds.lock().unwrap();
                        let mut occurrences = HashMap::<Vec<String>, usize>::new();
                        for message in messages {
                            if message["role"] != "assistant" {
                                continue;
                            }
                            if let Some(ids) = call_ids(message) {
                                let index = occurrences.entry(ids.clone()).or_default();
                                if let Some(original) =
                                    rounds.get(&ids).and_then(|items| items.get(*index))
                                {
                                    *message = original.clone();
                                }
                                *index += 1;
                            }
                        }
                    }
                    parts.headers.remove("content-length");
                    Bytes::from(to_vec(&body).expect("JSON serializes"))
                }
                Err(_) => bytes,
            }
        } else {
            bytes
        };
        Request::from_parts(parts, bytes)
    }

    fn remember(rounds: &Mutex<HashMap<Vec<String>, Vec<Value>>>, bytes: &Bytes) {
        let Ok(body) = from_slice::<Value>(bytes) else {
            return;
        };
        // Albert consumes the first choice, as does rig's OpenRouter adapter.
        let Some(message) = body.pointer("/choices/0/message") else {
            return;
        };
        if let Some(ids) = call_ids(message) {
            rounds
                .lock()
                .unwrap()
                .entry(ids)
                .or_default()
                .push(message.clone());
        }
    }
}

impl HttpClientExt for OpenRouterHttp {
    fn send<T, U>(
        &self,
        request: Request<T>,
    ) -> impl Future<Output = HttpResult<Response<LazyBody<U>>>> + WasmCompatSend + 'static
    where
        T: Into<Bytes> + WasmCompatSend,
        U: From<Bytes> + WasmCompatSend + 'static,
    {
        let capture = request.uri().path().ends_with("/chat/completions");
        let future = self.inner.send::<Bytes, Bytes>(self.restore(request));
        let rounds = self.rounds.clone();
        async move {
            let response = future.await?;
            let (parts, body) = response.into_parts();
            let bytes = body.await?;
            if capture {
                Self::remember(&rounds, &bytes);
            }
            let body: LazyBody<U> = Box::pin(async move { Ok(U::from(bytes)) });
            Ok(Response::from_parts(parts, body))
        }
    }

    fn send_multipart<U>(
        &self,
        request: Request<MultipartForm>,
    ) -> impl Future<Output = HttpResult<Response<LazyBody<U>>>> + WasmCompatSend + 'static
    where
        U: From<Bytes> + WasmCompatSend + 'static,
    {
        self.inner.send_multipart(request)
    }

    fn send_streaming<T>(
        &self,
        _request: Request<T>,
    ) -> impl Future<Output = HttpResult<StreamingResponse>> + WasmCompatSend
    where
        T: Into<Bytes>,
    {
        // Albert's API-key tool loop uses completion(), not streaming. Never
        // silently lose opaque state if a future caller changes that contract.
        async {
            Err(HttpError::Instance(
                "OpenRouter tool-round capture requires non-streaming requests".into(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn opaque_rounds_are_restored_exactly_and_never_cross_attempts() {
        for format in [
            "anthropic-claude-v1",
            "google-gemini-v1",
            "openai-responses-v1",
            "unknown-future-format",
        ] {
            let client = OpenRouterHttp::new(ReqwestClient::new());
            let original = json!({"role":"assistant","content":null,"tool_calls":[{"id":"call_a","type":"function","function":{"name":"read","arguments":"{}"}}],
                "reasoning_details":[{"type":"reasoning.encrypted","format":format,"index":7,"id":"opaque-id","data":"opaque-state"}],"provider_extension":{"untouched":true}});
            let response = Bytes::from(to_vec(&json!({"choices":[{"message":original}]})).unwrap());
            OpenRouterHttp::remember(&client.rounds, &response);
            let reduced =
                json!({"role":"assistant","content":[],"tool_calls":original["tool_calls"]});
            let make_request = || {
                Request::builder()
                    .uri("http://localhost/chat/completions")
                    .body(to_vec(&json!({"messages":[reduced]})).unwrap())
                    .unwrap()
            };
            let restored: Value = from_slice(client.restore(make_request()).body()).unwrap();
            assert_eq!(restored["messages"][0], original);
            let other = OpenRouterHttp::new(ReqwestClient::new());
            let clean: Value = from_slice(other.restore(make_request()).body()).unwrap();
            assert_eq!(clean["messages"][0], reduced);
        }
    }
    #[tokio::test]
    async fn tool_loops_round_trip_all_gateway_reasoning_shapes_without_reconstruction() {
        use axum::{routing::post, serve, Json, Router};
        use rig::{
            client::CompletionClient,
            completion::{Prompt, ToolDefinition},
            providers::openrouter,
            tool::Tool,
        };
        use std::{convert::Infallible, time::Duration};
        use tokio::{net::TcpListener, spawn, time::timeout};

        #[derive(Clone)]
        struct Probe;
        impl Tool for Probe {
            const NAME: &'static str = "probe";
            type Error = Infallible;
            type Args = Value;
            type Output = Value;
            async fn definition(&self, _: String) -> ToolDefinition {
                ToolDefinition {
                    name: "probe".into(),
                    description: "test".into(),
                    parameters: json!({"type":"object","properties":{}}),
                }
            }
            async fn call(&self, _: Value) -> Result<Value, Infallible> {
                Ok(json!({"ok":true}))
            }
        }
        for family in ["deepseek", "anthropic", "gemini", "openai"] {
            let mut original = json!({"role":"assistant","content":null,"tool_calls":[{"id":"call_probe","type":"function","function":{"name":"probe","arguments":"{}"}}]});
            match family {
                "deepseek" => original["reasoning"] = json!("provider-local reasoning"),
                "anthropic" => {
                    original["reasoning_details"] = json!([{"type":"reasoning.text","text":"thinking","signature":"opaque-signature","format":"anthropic-claude-v1","index":3}])
                }
                "gemini" => {
                    original["reasoning_details"] = json!([{"type":"reasoning.encrypted","data":"opaque-thought-state","id":"call_probe","format":"google-gemini-v1","index":0}]);
                    original["tool_calls"][0]["extra_content"] =
                        json!({"google":{"thought_signature":"opaque-part-signature"}});
                }
                _ => {
                    original["reasoning_details"] = json!([{"type":"reasoning.encrypted","data":"opaque-state","id":"rs_probe","format":"openai-responses-v1","index":4}])
                }
            }
            let requests = Arc::new(Mutex::new(Vec::<Value>::new()));
            let seen = requests.clone();
            let first = original.clone();
            let app=Router::new().route("/chat/completions",post(move |Json(body):Json<Value>| {
                let mut requests=seen.lock().unwrap();requests.push(body);
                let first_call=requests.len()==1;
                let message=if first_call {first.clone()} else {json!({"role":"assistant","content":"done"})};
                async move {Json(json!({"id":"test","object":"chat.completion","created":1,"model":"test","choices":[{"index":0,"message":message,"finish_reason":if first_call {"tool_calls"} else {"stop"}}]}))}
            }));
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let server = spawn(async move { serve(listener, app).await.unwrap() });
            let client = openrouter::Client::builder()
                .api_key("test")
                .base_url(&url)
                .http_client(OpenRouterHttp::new(ReqwestClient::new()))
                .build()
                .unwrap();
            let answer = timeout(
                Duration::from_secs(3),
                client.agent("test").tool(Probe).build().prompt("run probe"),
            )
            .await
            .unwrap()
            .unwrap();
            server.abort();
            let _ = server.await;
            assert_eq!(answer, "done");
            let requests = requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            let replay = requests[1]["messages"]
                .as_array()
                .unwrap()
                .iter()
                .find(|m| m["tool_calls"].as_array().is_some_and(|a| !a.is_empty()))
                .unwrap();
            assert_eq!(*replay, original, "{family}");
        }
    }
    #[test]
    fn repeated_call_ids_do_not_overwrite_earlier_rounds() {
        let client = OpenRouterHttp::new(ReqwestClient::new());
        let rounds=["first","second"].map(|value| json!({"role":"assistant","content":null,"tool_calls":[{"id":"same-id","type":"function","function":{"name":"probe","arguments":"{}"}}],"reasoning":value}));
        for round in &rounds {
            OpenRouterHttp::remember(
                &client.rounds,
                &Bytes::from(to_vec(&json!({"choices":[{"message":round}]})).unwrap()),
            );
        }
        let reduced = json!({"role":"assistant","tool_calls":rounds[0]["tool_calls"]});
        let request = Request::builder()
            .uri("http://localhost/chat/completions")
            .body(to_vec(&json!({"messages":[reduced.clone(),reduced]})).unwrap())
            .unwrap();
        let body: Value = from_slice(client.restore(request).body()).unwrap();
        assert_eq!(body["messages"], json!(rounds));
    }
}
