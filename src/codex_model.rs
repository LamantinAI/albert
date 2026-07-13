//! [`CodexResponsesModel`] — a rig [`CompletionModel`] for the ChatGPT-**subscription**
//! Codex backend (`https://chatgpt.com/backend-api/codex/responses`).
//!
//! It wraps rig's own OpenAI Responses model and reconciles the one way the Codex
//! endpoint differs at the model layer: it is **streaming-only** — a non-streaming
//! request is rejected (`400 "Stream must be set to true"`). rig's
//! [`CompletionModel::completion`] is single-shot, so we route it through rig's
//! streaming path (which forces `stream:true`) and fold the SSE back into a
//! [`CompletionResponse`] — the agent tool-loop never knows it streamed.
//!
//! The other Codex quirk (no `system`-role messages) is handled one layer down, at
//! the transport, by [`crate::codex_http::CodexHttp`], so it stays out of here.

use futures::StreamExt;
use rig::{
    client::CompletionClient,
    completion::{
        CompletionError, CompletionModel, CompletionRequest, CompletionResponse, GetTokenUsage,
        Usage,
    },
    providers::openai::{responses_api::ResponsesCompletionModel, Client},
    streaming::StreamingCompletionResponse,
};

use crate::codex_http::CodexHttp;

/// rig's Responses model over our body-rewriting [`CodexHttp`] transport, wrapped
/// to turn the tool-loop's non-streaming `completion()` into a streaming request.
#[derive(Clone)]
pub struct CodexResponsesModel {
    inner: ResponsesCompletionModel<CodexHttp>,
}

impl CompletionModel for CodexResponsesModel {
    // The agent tool-loop reads only `choice`/`usage`/`message_id`, never the raw
    // provider payload, and the streaming path yields no single JSON body to fill
    // it with — so `()` is both sufficient and simplest.
    type Response = ();
    type StreamingResponse = <ResponsesCompletionModel<CodexHttp> as CompletionModel>::StreamingResponse;
    type Client = Client<CodexHttp>;

    fn make(client: &Self::Client, model: impl Into<String>) -> Self {
        Self { inner: client.completion_model(model) }
    }

    async fn completion(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse<Self::Response>, CompletionError> {
        // Delegate to the STREAMING path so rig sends `stream:true` and parses the
        // SSE for us; then drain it. rig folds every text delta and tool call into
        // `choice` on the terminal poll (the `None` that ends this loop).
        let mut stream = CompletionModel::stream(&self.inner, request).await?;
        while let Some(item) = stream.next().await {
            item?; // propagate provider / parse errors; side effects do the folding
        }
        let usage = stream
            .response
            .as_ref()
            .and_then(GetTokenUsage::token_usage)
            .unwrap_or_else(Usage::new);
        Ok(CompletionResponse {
            choice: stream.choice.clone(),
            usage,
            raw_response: (),
            message_id: stream.message_id.clone(),
        })
    }

    async fn stream(
        &self,
        request: CompletionRequest,
    ) -> Result<StreamingCompletionResponse<Self::StreamingResponse>, CompletionError> {
        CompletionModel::stream(&self.inner, request).await
    }
}

#[cfg(test)]
mod live {
    //! Live end-to-end check against the real Codex backend, reusing the local
    //! `codex` login. Ignored by default (hits the network + spends a little
    //! subscription quota). Run explicitly:
    //!   cargo test --bin albert codex_live -- --ignored --nocapture
    //! It proves the whole subscription path works: the request streams (else a
    //! "Stream must be set to true" 400) and the preamble is accepted (else a
    //! "System messages are not allowed" 400) — verified by a marker instruction
    //! the model must echo, which also proves the (rewritten) system prompt lands.

    use std::{env::var, fs::read_to_string};

    use rig::{
        agent::AgentBuilder,
        completion::{CompletionModel, Prompt},
        http_client::{HeaderMap, HeaderValue},
        providers::openai,
    };
    use serde_json::{from_str, Value};

    use super::CodexResponsesModel;
    use crate::codex_http::CodexHttp;

    #[tokio::test]
    #[ignore = "hits the real Codex endpoint; run with --ignored"]
    async fn codex_live_instructions_and_streaming() {
        let path = format!("{}/.codex/auth.json", var("HOME").unwrap());
        let v: Value = from_str(&read_to_string(&path).unwrap()).unwrap();
        let token = v["tokens"]["access_token"].as_str().unwrap();
        let account = v["tokens"]["account_id"].as_str().unwrap();

        let mut headers = HeaderMap::new();
        headers.insert("chatgpt-account-id", HeaderValue::from_str(account).unwrap());
        headers.insert("originator", HeaderValue::from_static("codex_cli_rs"));
        let base = var("CODEX_TEST_BASE")
            .unwrap_or_else(|_| "https://chatgpt.com/backend-api/codex".to_string());
        let client = openai::Client::builder()
            .base_url(&base)
            .api_key(token)
            .http_headers(headers)
            .http_client(CodexHttp::default())
            .build()
            .unwrap();

        let model = CodexResponsesModel::make(&client, "gpt-5.4-mini");
        let agent = AgentBuilder::new(model)
            .preamble(
                "You are a strict test bot. Reply with exactly the single word BANANA \
                 and nothing else, whatever the user says.",
            )
            .build();
        let out = agent
            .prompt("Say hello and tell me today's weather.")
            .await
            .expect("codex completion");
        println!("codex live reply: {out:?}");
        assert!(out.contains("BANANA"), "instructions not honored; got: {out:?}");
    }
}
