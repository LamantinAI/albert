use std::sync::Arc;

use rig::{
    completion::{
        CompletionError, CompletionModel, CompletionRequest, CompletionResponse, Message,
    },
    streaming::StreamingCompletionResponse,
};
use serde_json::{json, to_string};
use tracing::info;

use super::{recovery::Recovery, Settings};
use crate::{status::StatusFeed, subagents::budget::Budget};

/// Every model round is checked, including tool continuations and fallback.
#[derive(Clone)]
pub struct BudgetedModel<M> {
    inner: M,
    settings: Option<Settings>,
    api_output_cap: bool,
    turn_budget: Option<Arc<Budget>>,
    recovery: Option<Recovery>,
}
impl<M> BudgetedModel<M> {
    pub fn new(inner: M, settings: Option<Settings>, api_output_cap: bool) -> Self {
        Self {
            inner,
            settings,
            api_output_cap,
            turn_budget: None,
            recovery: None,
        }
    }
    pub fn with_recovery(mut self, retries: usize, delay_ms: u64, feed: StatusFeed) -> Self {
        self.recovery = Some(Recovery::new(retries, delay_ms, feed));
        self
    }
    pub fn with_turn_budget(mut self, budget: Option<Arc<Budget>>) -> Self {
        self.turn_budget = budget;
        self
    }
    fn guard(&self, request: &mut CompletionRequest) -> Result<(), CompletionError> {
        if let Some(budget) = &self.turn_budget {
            budget.prepare(request)?;
        }
        let Some(settings) = &self.settings else {
            self.output_cap(request);
            return Ok(());
        };
        let mut instructions = settings.count(request.preamble.as_deref().unwrap_or(""));
        instructions += settings.count(&to_string(&request.tools).unwrap_or_default());
        instructions += settings.count(&to_string(&request.additional_params).unwrap_or_default());
        let mut dialogue = 0;
        for message in request.chat_history.iter() {
            if matches!(message, Message::System { .. }) {
                instructions += settings.message_tokens(message);
            } else {
                dialogue += settings.message_tokens(message);
            }
        }
        if let Some(documents) = request.normalized_documents() {
            dialogue += settings.message_tokens(&documents);
        }
        let output = request
            .max_tokens
            .unwrap_or(settings.response_tokens as u64)
            .min(settings.response_tokens as u64);
        request.max_tokens = Some(output);
        info!(
            instructions,
            dialogue,
            output,
            reserve = settings.reserve_tokens,
            window = settings.window_tokens,
            "estimated context budget"
        );
        if instructions.saturating_add(output as usize) > settings.reserve_tokens {
            return Err(CompletionError::ProviderError("ALBERT_CONTEXT_RESERVE: instructions, tools and response exceed the configured fixed reserve".into()));
        }
        if settings.dialogue() == 0 || dialogue > settings.dialogue() {
            return Err(CompletionError::ProviderError("ALBERT_CONTEXT_WINDOW: dialogue exceeds its configured partition; compact or reduce the oversized input/tool result".into()));
        }
        self.output_cap(request);
        Ok(())
    }
    fn output_cap(&self, request: &mut CompletionRequest) {
        if self.api_output_cap {
            if let Some(limit) = request.max_tokens {
                // rig OpenRouter currently ignores CompletionRequest.max_tokens;
                // its flattened parameters are the supported transport path.
                let params = request.additional_params.get_or_insert_with(|| json!({}));
                params["max_tokens"] = json!(limit);
            }
        } else {
            // Subscription Responses does not accept an output-cap parameter.
            request.max_tokens = None;
        }
    }
}
impl<M: CompletionModel> CompletionModel for BudgetedModel<M> {
    type Response = M::Response;
    type StreamingResponse = M::StreamingResponse;
    type Client = Self;
    fn make(client: &Self, _: impl Into<String>) -> Self {
        client.clone()
    }
    async fn completion(
        &self,
        mut request: CompletionRequest,
    ) -> Result<CompletionResponse<Self::Response>, CompletionError> {
        self.guard(&mut request)?;
        let response = match &self.recovery {
            Some(recovery) => recovery.complete(&self.inner, request).await?,
            None => self.inner.completion(request).await?,
        };
        if let Some(budget) = &self.turn_budget {
            budget.record(&response.choice);
        }
        if self.settings.is_some() {
            info!(
                input_tokens = response.usage.input_tokens,
                output_tokens = response.usage.output_tokens,
                "provider context usage"
            );
        }
        Ok(response)
    }
    async fn stream(
        &self,
        mut request: CompletionRequest,
    ) -> Result<StreamingCompletionResponse<Self::StreamingResponse>, CompletionError> {
        self.guard(&mut request)?;
        self.inner.stream(request).await
    }
}
