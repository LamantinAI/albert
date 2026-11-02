use std::time::{Duration, Instant};

use futures::StreamExt;
use rig::{
    completion::{
        CompletionError, CompletionModel, CompletionRequest, CompletionResponse, GetTokenUsage,
        Usage,
    },
    message::AssistantContent,
    streaming::{StreamedAssistantContent, StreamingCompletionResponse},
};

use crate::status::StatusFeed;

/// Observe provider text below budget/recovery, retaining the ordinary rig loop.
#[derive(Clone)]
pub(crate) struct ObservedModel<M> {
    inner: M,
    feed: Option<StatusFeed>,
}
impl<M> ObservedModel<M> {
    pub fn new(inner: M, feed: Option<StatusFeed>) -> Self {
        Self { inner, feed }
    }
}
impl<M: CompletionModel> CompletionModel for ObservedModel<M> {
    type Response = ();
    type StreamingResponse = M::StreamingResponse;
    type Client = Self;
    fn make(client: &Self, _: impl Into<String>) -> Self {
        client.clone()
    }
    async fn completion(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse<()>, CompletionError> {
        let Some(feed) = self.feed.as_ref().filter(|feed| feed.text_enabled()) else {
            let response = self.inner.completion(request).await?;
            return Ok(CompletionResponse {
                choice: response.choice,
                usage: response.usage,
                message_id: response.message_id,
                raw_response: (),
            });
        };
        let generation = feed.text_begin().await;
        let result = async {
            let mut stream = self.inner.stream(request).await?;
            let mut pending = String::new();
            let mut last = Instant::now();
            let mut emitted = false;
            let mut terminal = false;
            while let Some(chunk) = stream.next().await {
                match chunk? {
                    StreamedAssistantContent::Text(text) => {
                        pending.push_str(&text.text);
                        if !emitted
                            || pending.len() >= 1024
                            || last.elapsed() >= Duration::from_millis(40)
                        {
                            feed.text_append(&generation, &pending).await;
                            pending.clear();
                            emitted = true;
                            last = Instant::now();
                        }
                    }
                    StreamedAssistantContent::Final(_) => terminal = true,
                    _ => {}
                }
            }
            if !terminal {
                return Err(CompletionError::ProviderError(
                    "Provider stream ended before completion.".into(),
                ));
            }
            if !pending.is_empty() {
                feed.text_append(&generation, &pending).await;
            }
            if stream
                .choice
                .iter()
                .any(|item| matches!(item, AssistantContent::ToolCall(_)))
            {
                feed.text_reset(&generation).await;
            }
            let usage = stream
                .response
                .as_ref()
                .and_then(GetTokenUsage::token_usage)
                .unwrap_or_else(Usage::new);
            Ok(CompletionResponse {
                choice: stream.choice.clone(),
                usage,
                message_id: stream.message_id.clone(),
                raw_response: (),
            })
        }
        .await;
        if result.is_err() {
            feed.text_reset(&generation).await;
        }
        result
    }
    async fn stream(
        &self,
        request: CompletionRequest,
    ) -> Result<StreamingCompletionResponse<Self::StreamingResponse>, CompletionError> {
        self.inner.stream(request).await
    }
}
