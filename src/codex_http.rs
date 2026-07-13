//! A rig [`HttpClientExt`] that makes rig's OpenAI-Responses request body legal
//! for the ChatGPT-subscription **Codex** backend, at the transport layer.
//!
//! The Codex `/responses` endpoint rejects a `system`-role item in `input`
//! (`400 "System messages are not allowed"`) but accepts a `developer`-role item,
//! which carries the same instruction weight. rig always serializes the agent
//! preamble as a `system` message and offers no public hook to change that, so we
//! rewrite the JSON body just before it goes on the wire: `system` -> `developer`
//! on every `input[]` item. Everything else (headers, auth, streaming, and — key —
//! rig's response/SSE and tool-call parsing) is reused untouched; only the
//! `POST .../responses` body is edited, and only when it is the shape we expect.

use bytes::Bytes;
use rig::{
    http_client::{
        HeaderValue, HttpClientExt, LazyBody, MultipartForm, ReqwestClient, Request, Response,
        Result as HttpResult, StreamingResponse,
    },
    wasm_compat::WasmCompatSend,
};
use serde_json::{from_slice, to_vec, Value};
use std::{collections::HashSet, future::Future};

/// reqwest transport that rewrites the Codex request body (`system` -> `developer`).
#[derive(Clone, Default, Debug)]
pub struct CodexHttp {
    inner: ReqwestClient,
}

impl CodexHttp {
    /// Rewrite the body of a `POST .../responses` so every `system`-role input
    /// item becomes `developer`; leave every other request (and any unrecognized
    /// body) untouched.
    fn reshape<T: Into<Bytes>>(req: Request<T>) -> Request<Bytes> {
        let (mut parts, body) = req.into_parts();
        let bytes: Bytes = body.into();
        let bytes = if parts.uri.path().ends_with("/responses") {
            match codexify_body(&bytes) {
                // The edit changes the body length, so the original `Content-Length`
                // is now stale — drop it and let reqwest recompute from the new body.
                Some(edited) => {
                    parts.headers.remove("content-length");
                    edited
                }
                None => bytes,
            }
        } else {
            bytes
        };
        Request::from_parts(parts, bytes)
    }
}

/// Make rig's Responses body Codex-legal. Three edits (all of which the Codex
/// backend rejects a request without, but the stock OpenAI Responses API tolerates):
///
/// - force `store:false` — Codex rejects a stored request, and rig never sends the
///   field at all;
/// - `system` -> `developer` on every `input[]` item — Codex forbids `system`
///   messages but accepts `developer`, which carries the same instruction weight;
/// - relax the tool schemas — rig marks every tool `strict:true`, but Codex then
///   enforces OpenAI strict-function rules that these tools don't satisfy (e.g.
///   `dispatch_to_connector` lists a `required` key absent from `properties`). We
///   set `strict:false` and drop `required` entries that name no real property, so
///   the schemas validate while keeping their genuinely-optional fields optional.
///
/// Returns `None` if the body isn't the JSON object we expect — the caller then
/// keeps the original bytes verbatim.
fn codexify_body(bytes: &Bytes) -> Option<Bytes> {
    let mut body: Value = from_slice(bytes).ok()?;
    let obj = body.as_object_mut()?;
    obj.insert("store".to_string(), Value::Bool(false));
    if let Some(input) = obj.get_mut("input").and_then(Value::as_array_mut) {
        for item in input.iter_mut() {
            if item.get("role").and_then(Value::as_str) == Some("system") {
                item["role"] = Value::String("developer".to_string());
            }
        }
    }
    if let Some(tools) = obj.get_mut("tools").and_then(Value::as_array_mut) {
        for tool in tools.iter_mut() {
            relax_tool_schema(tool);
        }
    }
    Some(Bytes::from(to_vec(&body).ok()?))
}

/// `strict:false` + prune phantom `required` keys on one tool definition.
fn relax_tool_schema(tool: &mut Value) {
    let Some(tool) = tool.as_object_mut() else { return };
    tool.insert("strict".to_string(), Value::Bool(false));
    let Some(params) = tool.get_mut("parameters").and_then(Value::as_object_mut) else { return };
    let props: HashSet<String> = params
        .get("properties")
        .and_then(Value::as_object)
        .map(|p| p.keys().cloned().collect())
        .unwrap_or_default();
    if let Some(required) = params.get_mut("required").and_then(Value::as_array_mut) {
        required.retain(|k| k.as_str().is_some_and(|s| props.contains(s)));
    }
}

impl HttpClientExt for CodexHttp {
    fn send<T, U>(
        &self,
        req: Request<T>,
    ) -> impl Future<Output = HttpResult<Response<LazyBody<U>>>> + WasmCompatSend + 'static
    where
        T: Into<Bytes> + WasmCompatSend,
        U: From<Bytes> + WasmCompatSend + 'static,
    {
        self.inner.send::<Bytes, U>(Self::reshape(req))
    }

    fn send_multipart<U>(
        &self,
        req: Request<MultipartForm>,
    ) -> impl Future<Output = HttpResult<Response<LazyBody<U>>>> + WasmCompatSend + 'static
    where
        U: From<Bytes> + WasmCompatSend + 'static,
    {
        self.inner.send_multipart::<U>(req)
    }

    fn send_streaming<T>(
        &self,
        req: Request<T>,
    ) -> impl Future<Output = HttpResult<StreamingResponse>> + WasmCompatSend
    where
        T: Into<Bytes>,
    {
        let fut = self.inner.send_streaming::<Bytes>(Self::reshape(req));
        async move {
            let mut resp = fut.await?;
            // The Codex SSE response ships no `Content-Type` header at all; rig's
            // event parser rejects an empty content type, so assert the SSE type
            // the stream actually is (only when the server left it unset).
            resp.headers_mut()
                .entry("content-type")
                .or_insert(HeaderValue::from_static("text/event-stream"));
            Ok(resp)
        }
    }
}
