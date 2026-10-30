use std::{sync::Arc, time::Instant};

use rig::{
    completion::{
        CompletionError, CompletionModel, CompletionRequest, CompletionResponse, Message,
    },
    streaming::StreamingCompletionResponse,
};
use serde_json::{json, to_string, Value};
use tracing::{info, Instrument};

use super::{diagnostics::Diagnostics, recovery::Recovery, DiagnosticScope, Settings};
use crate::{status::StatusFeed, subagents::budget::Budget};

/// Every model round is checked, including tool continuations and fallback.
#[derive(Clone)]
pub struct BudgetedModel<M> {
    inner: M,
    settings: Option<Settings>,
    api_output_cap: bool,
    turn_budget: Option<Arc<Budget>>,
    recovery: Option<Recovery>,
    model_params: Option<Value>,
    diagnostics: Diagnostics,
    diagnostic_feed: Option<StatusFeed>,
}
impl<M> BudgetedModel<M> {
    pub fn new(inner: M, settings: Option<Settings>, api_output_cap: bool) -> Self {
        Self {
            inner,
            settings,
            api_output_cap,
            turn_budget: None,
            recovery: None,
            model_params: None,
            diagnostics: Diagnostics::default(),
            diagnostic_feed: None,
        }
    }
    pub(crate) fn with_diagnostics(mut self, scope: Option<DiagnosticScope>) -> Self {
        self.diagnostics.scope = scope;
        self
    }
    pub(crate) fn with_diagnostic_model(mut self, model: &str) -> Self {
        self.diagnostics.model = Some(model.into());
        self
    }
    pub(crate) fn with_diagnostic_feed(mut self, feed: StatusFeed) -> Self {
        self.diagnostic_feed = Some(feed);
        self
    }
    pub fn with_model_params(mut self, params: Option<Value>) -> Self {
        self.model_params = params;
        self
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
        if let Some(Value::Object(params)) = &self.model_params {
            let target = request.additional_params.get_or_insert_with(|| json!({}));
            let target = target.as_object_mut().ok_or_else(|| {
                CompletionError::ProviderError("Model parameters must be an object".into())
            })?;
            for (key, value) in params {
                target.insert(key.clone(), value.clone());
            }
        }
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
        let round = self.diagnostics.next("completion");
        let request_id = round.id;
        let span = round.span;
        async move {
            let started = Instant::now();
            if let Err(error) = self.guard(&mut request) {
                info!(guard_ms = elapsed_ms(started), provider_called = false, "model request rejected before provider");
                return Err(error);
            }
            let guard_ms = elapsed_ms(started);
            let cap = request.max_tokens;
            if let Some(feed) = &self.diagnostic_feed {
                feed.set_request_id(&request_id);
            }
            let provider_started = Instant::now();
            // Includes client recovery/backoff, transport, provider queues and
            // compute. This is not a measurement of pure inference or TTFB.
            let result = match &self.recovery {
                Some(recovery) => recovery.complete(&self.inner, request).await,
                None => self.inner.completion(request).await,
            };
            let provider_round_wall_ms = elapsed_ms(provider_started);
            match result {
                Ok(response) => {
                    if let Some(budget) = &self.turn_budget {
                        budget.record(&response.choice);
                    }
                    if self.settings.is_some() || self.diagnostics.scope.is_some() {
                        info!(guard_ms, provider_round_wall_ms,
                            recovery_enabled = self.recovery.is_some(), provider_called = true,
                            input_tokens = response.usage.input_tokens,
                            output_tokens = response.usage.output_tokens,
                            reported_cached_input_tokens = response.usage.cached_input_tokens,
                            applied_output_cap = ?cap,
                            applied_output_cap_hit = ?cap.map(|limit| response.usage.output_tokens >= limit),
                            "provider context usage");
                    }
                    Ok(response)
                }
                Err(error) => {
                    info!(guard_ms, provider_round_wall_ms, provider_called = true,
                        recovery_enabled = self.recovery.is_some(), "provider completion round failed");
                    Err(error)
                }
            }
        }.instrument(span).await
    }
    async fn stream(
        &self,
        mut request: CompletionRequest,
    ) -> Result<StreamingCompletionResponse<Self::StreamingResponse>, CompletionError> {
        let round = self.diagnostics.next("stream_setup");
        let request_id = round.id;
        let span = round.span;
        async move {
            let started = Instant::now();
            if let Err(error) = self.guard(&mut request) {
                info!(
                    guard_ms = elapsed_ms(started),
                    provider_called = false,
                    "model stream rejected before provider"
                );
                return Err(error);
            }
            let guard_ms = elapsed_ms(started);
            if let Some(feed) = &self.diagnostic_feed {
                feed.set_request_id(&request_id);
            }
            let setup = Instant::now();
            let result = self.inner.stream(request).await;
            info!(
                guard_ms,
                stream_setup_wall_ms = elapsed_ms(setup),
                succeeded = result.is_ok(),
                stream_completion_observed = false,
                "model stream setup finished"
            );
            result
        }
        .instrument(span)
        .await
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u64::MAX as u128) as u64
}
