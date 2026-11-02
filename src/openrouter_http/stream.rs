use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex},
};

use bytes::Bytes;
use futures::{stream::unfold, StreamExt};
use rig::http_client::{sse::BoxedStream, Error as HttpError};
use serde_json::{from_slice, json, to_vec, Map, Value};

use super::OpenRouterHttp;
use crate::transport::{incomplete, normalize};

/// Validate wire termination before the SDK can synthesize success at EOF, and
/// retain the protocol's original assistant round for same-attempt tool replay.
pub(super) fn guarded(
    body: BoxedStream,
    rounds: Arc<Mutex<HashMap<Vec<String>, Vec<Value>>>>,
) -> BoxedStream {
    Box::pin(unfold(
        (body, Capture::default(), false, rounds),
        |(mut body, mut capture, finished, rounds)| async move {
            if finished {
                return capture
                    .take_usage()
                    .map(|bytes| (Ok(bytes), (body, capture, true, rounds)));
            }
            match body.next().await {
                Some(Ok(bytes)) => {
                    if let Err(error) = capture.observe(&bytes) {
                        return Some((Err(error), (body, capture, true, rounds)));
                    }
                    let done = capture.done;
                    if done {
                        capture.remember(&rounds);
                    }
                    Some((Ok(bytes), (body, capture, done, rounds)))
                }
                Some(Err(error)) => Some((Err(normalize(error)), (body, capture, true, rounds))),
                None if capture.terminal => {
                    capture.remember(&rounds);
                    capture
                        .take_usage()
                        .map(|bytes| (Ok(bytes), (body, capture, true, rounds)))
                }
                None => Some((Err(incomplete()), (body, capture, true, rounds))),
            }
        },
    ))
}
#[derive(Default)]
struct Capture {
    line: Vec<u8>,
    data: Vec<u8>,
    done: bool,
    terminal: bool,
    bytes: usize,
    message: Map<String, Value>,
    calls: BTreeMap<u64, Value>,
    reasoning: BTreeMap<u64, Value>,
    usage: Option<Value>,
}
fn append(target: &mut Value, delta: &Value, key: &str) {
    let Some(delta) = delta.get(key).and_then(Value::as_str) else {
        return;
    };
    let old = target.get(key).and_then(Value::as_str).unwrap_or("");
    target[key] = Value::String(format!("{old}{delta}"));
}
impl Capture {
    fn take_usage(&mut self) -> Option<Bytes> {
        self.usage.take().map(|mut value| {
            // The pinned SDK skips usage-only OpenAI chunks. Feed it a neutral
            // choice after validated termination; no text or tools are invented.
            value["choices"] = json!([{"index":0,"delta":{},"finish_reason":null}]);
            Bytes::from(format!("data: {value}\n\n"))
        })
    }
    fn observe(&mut self, bytes: &Bytes) -> Result<(), HttpError> {
        self.bytes += bytes.len();
        if self.bytes > 8 * 1024 * 1024 {
            return Err(HttpError::Instance(
                "Provider stream exceeds capture limit".into(),
            ));
        }
        for piece in bytes.split_inclusive(|b| *b == b'\n') {
            self.line.extend_from_slice(piece);
            if piece.last() != Some(&b'\n') {
                continue;
            }
            let line = self.line.strip_suffix(b"\n").unwrap_or(&self.line);
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            if line.is_empty() {
                if self.data.strip_suffix(b"\n").unwrap_or(&self.data) == b"[DONE]" {
                    self.done = true;
                    self.terminal = true;
                } else if !self.data.is_empty() {
                    let value = from_slice::<Value>(&self.data)
                        .map_err(|_| HttpError::Instance("Invalid provider stream event".into()))?;
                    self.event(value)?;
                }
                if self.done {
                    self.validate()?;
                    return Ok(());
                }
                self.data.clear();
            } else if let Some(data) = line.strip_prefix(b"data:") {
                self.data
                    .extend_from_slice(data.strip_prefix(b" ").unwrap_or(data));
                self.data.push(b'\n');
            }
            self.line.clear();
        }
        Ok(())
    }
    fn event(&mut self, value: Value) -> Result<(), HttpError> {
        if let Some(error) = value.get("error") {
            let code = error
                .get("code")
                .or_else(|| error.get("status"))
                .and_then(Value::as_u64)
                .filter(|code| (400..=599).contains(code));
            return Err(HttpError::Instance(
                code.map(|code| format!("Provider streaming status {code}"))
                    .unwrap_or_else(|| {
                        "ALBERT_TRANSPORT_CONNECTION: provider streaming error".into()
                    })
                    .into(),
            ));
        }
        let Some(choice) = value.pointer("/choices/0") else {
            if value.get("usage").is_some_and(Value::is_object) {
                self.usage = Some(value);
            }
            return Ok(());
        };
        if value.get("usage").is_some_and(Value::is_object) {
            self.usage = None;
        }
        self.terminal |= choice.get("finish_reason").is_some_and(|v| !v.is_null());
        let Some(delta) = choice.get("delta").and_then(Value::as_object) else {
            return Ok(());
        };
        for (key, value) in delta {
            match key.as_str() {
                "content" | "reasoning" | "reasoning_content" if value.is_string() => {
                    let old = self.message.get(key).and_then(Value::as_str).unwrap_or("");
                    self.message.insert(
                        key.clone(),
                        Value::String(format!("{old}{}", value.as_str().unwrap_or(""))),
                    );
                }
                "content" | "reasoning" | "reasoning_content" if value.is_null() => {}
                "tool_calls" => {
                    if let Some(calls) = value.as_array() {
                        for (position, call) in calls.iter().enumerate() {
                            let index = call
                                .get("index")
                                .and_then(Value::as_u64)
                                .unwrap_or(position as u64);
                            if index > 1024 {
                                return Err(HttpError::Instance(
                                    "Invalid provider call index".into(),
                                ));
                            }
                            let target = self.calls.entry(index).or_insert_with(
                                || json!({"type":"function","function":{"name":"","arguments":""}}),
                            );
                            if let Some(id) = call.get("id") {
                                target["id"] = id.clone();
                            }
                            if let Some(function) = call.get("function") {
                                append(&mut target["function"], function, "name");
                                append(&mut target["function"], function, "arguments");
                                if let Some(object) = function.as_object() {
                                    for (k, v) in object {
                                        if !["name", "arguments"].contains(&k.as_str()) {
                                            target["function"][k] = v.clone();
                                        }
                                    }
                                }
                            }
                            if let Some(object) = call.as_object() {
                                for (k, v) in object {
                                    if !["index", "id", "function"].contains(&k.as_str()) {
                                        target[k] = v.clone();
                                    }
                                }
                            }
                        }
                    }
                }
                "reasoning_details" => {
                    if let Some(details) = value.as_array() {
                        for (position, detail) in details.iter().enumerate() {
                            let index = detail
                                .get("index")
                                .and_then(Value::as_u64)
                                .unwrap_or(position as u64);
                            if index > 1024 {
                                return Err(HttpError::Instance(
                                    "Invalid provider reasoning index".into(),
                                ));
                            }
                            let target = self.reasoning.entry(index).or_insert_with(|| json!({}));
                            if let Some(object) = detail.as_object() {
                                for (k, v) in object {
                                    if ["text", "summary", "data"].contains(&k.as_str())
                                        && v.is_string()
                                    {
                                        append(target, detail, k);
                                    } else {
                                        target[k] = v.clone();
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {
                    self.message.insert(key.clone(), value.clone());
                }
            }
        }
        Ok(())
    }
    fn remember(&self, rounds: &Mutex<HashMap<Vec<String>, Vec<Value>>>) {
        if self.calls.is_empty() {
            return;
        }
        let mut message = self.message.clone();
        message.insert("role".into(), json!("assistant"));
        message.entry("content").or_insert(Value::Null);
        message.insert(
            "tool_calls".into(),
            Value::Array(self.calls.values().cloned().collect()),
        );
        if !self.reasoning.is_empty() {
            message.insert(
                "reasoning_details".into(),
                Value::Array(self.reasoning.values().cloned().collect()),
            );
        }
        OpenRouterHttp::remember(
            rounds,
            &Bytes::from(
                to_vec(&json!({"choices":[{"message":message}]}))
                    .expect("captured JSON serializes"),
            ),
        );
    }
    fn validate(&self) -> Result<(), HttpError> {
        for call in self.calls.values() {
            if call["id"].as_str().is_none_or(str::is_empty)
                || call["function"]["name"].as_str().is_none_or(str::is_empty)
                || call["function"]["arguments"]
                    .as_str()
                    .is_none_or(|args| from_slice::<Value>(args.as_bytes()).is_err())
            {
                return Err(HttpError::Instance(
                    "Invalid completed provider tool call".into(),
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream::iter;
    #[tokio::test]
    async fn incomplete_wire_never_becomes_success() {
        let body: BoxedStream = Box::pin(iter(vec![Ok(Bytes::from_static(
            b"data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
        ))]));
        let mut stream = guarded(body, Arc::new(Mutex::new(HashMap::new())));
        assert!(stream.next().await.unwrap().is_ok());
        assert!(stream.next().await.unwrap().is_err());
    }
    #[test]
    fn protocol_indexed_details_and_calls_survive_capture() {
        let mut capture = Capture::default();
        for chunk in [
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call","function":{"name":"probe","arguments":"{"}}],"reasoning_details":[{"index":7,"type":"reasoning.encrypted","format":"future","data":"opaque-"}]}}]}),
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"}"}}],"reasoning_details":[{"index":7,"data":"state"}]},"finish_reason":"tool_calls"}]}),
        ] {
            capture.event(chunk).unwrap();
        }
        let rounds = Mutex::new(HashMap::new());
        capture.remember(&rounds);
        let values = rounds.lock().unwrap();
        let message = &values[&vec!["call".to_owned()]][0];
        assert_eq!(message["reasoning_details"][0]["data"], "opaque-state");
        assert_eq!(message["tool_calls"][0]["function"]["arguments"], "{}");
    }
}
