//! Provider-specific model attempts and authenticated client construction.
use super::AttemptTools;
use std::{time::Duration, time::Instant};

use octo_openai_auth::Subscription as SubToken;
use rig::{
    agent::AgentBuilder, client::CompletionClient, completion::CompletionModel,
    completion::Message, completion::PromptError, http_client::HeaderMap, http_client::HeaderValue,
    http_client::ReqwestClient, providers::openai,
    providers::openrouter::Client as OpenRouterClient,
};
use tracing::{info, warn};

use crate::cogitator::{outcome::AgentOutcome, AlbertCogitator};
use crate::{
    codex_http::CodexHttp, codex_model::CodexResponsesModel, cogitator::errors::model_failure,
    config::AuthMode, context::BudgetedModel, models::Failure, models::FailureKind,
    models::ModelSpec, openrouter_http::OpenRouterHttp, status::text::ObservedModel,
    status::StatusFeed,
};

impl AlbertCogitator {
    pub(in crate::cogitator) async fn model_attempt_outcome(
        &self,
        model: &ModelSpec,
        force_refresh: bool,
        preamble: &str,
        tools: AttemptTools,
        channel: &str,
        prompt: Message,
        history: Vec<Message>,
        feed: StatusFeed,
    ) -> Result<AgentOutcome, Failure> {
        if !feed.is_live() {
            return Err(Failure {
                kind: FailureKind::Fatal,
                message: "Session ended.".into(),
            });
        }
        let unavailable = |message: &str| Failure {
            kind: FailureKind::Unavailable,
            message: message.into(),
        };
        let reasoning_params = model.reasoning_params().map_err(|message| Failure {
            kind: FailureKind::Incompatible,
            message,
        })?;
        let turn_budget = match &tools {
            AttemptTools::Child { budget, .. } => Some(budget.clone()),
            _ => None,
        };
        let compact = matches!(&tools, AttemptTools::Compact { .. });
        let metadata = matches!(&tools, AttemptTools::Metadata { .. });
        let diagnostics = feed.diagnostics_for(compact);
        let diagnostic_model_id = model.id.as_str();
        let timeout_ms = if compact {
            self.config.context.request_timeout_ms
        } else {
            model.request_timeout_ms
        };
        let started = Instant::now();
        let http = ReqwestClient::builder()
            .timeout(Duration::from_millis(timeout_ms))
            .build()
            .map_err(|_| unavailable("Could not construct HTTP client."))?;

        let mut budget = self.config.context.for_model(model);
        if let AttemptTools::Compact { max_tokens } | AttemptTools::Metadata { max_tokens } = &tools
        {
            budget.response_tokens = *max_tokens;
        }
        let budget = budget.enabled.then_some(budget);
        let accounting = self.session_service.as_ref().map(|service| {
            service.usage().recorder(
                model,
                if compact {
                    "compact"
                } else if metadata {
                    "metadata"
                } else if turn_budget.is_some() {
                    "subagent"
                } else {
                    "chat"
                },
            )
        });
        let preview = matches!(
            &tools,
            AttemptTools::Root(_) | AttemptTools::ChatOnly { .. }
        )
        .then(|| feed.clone());
        let control_feed = feed.clone();
        let result = match model.provider {
            AuthMode::ApiKey => {
                let key = self
                    .config
                    .model_key(model.api_key_env.as_deref())
                    .filter(|key| !key.trim().is_empty())
                    .ok_or_else(|| unavailable("API key is unavailable."))?;
                let mut builder = OpenRouterClient::builder()
                    .api_key(key.as_str())
                    .http_client(OpenRouterHttp::new(http));
                let base = model.base_url.as_deref().unwrap_or(&self.config.base_url);
                if !base.is_empty() {
                    builder = builder.base_url(base);
                }
                let client = builder
                    .build()
                    .map_err(|_| unavailable("Could not construct API client."))?;
                self.drive_attempt(
                    AgentBuilder::new(
                        BudgetedModel::new(
                            ObservedModel::new(client.completion_model(&model.model), preview)
                                .with_accounting(accounting),
                            budget,
                            true,
                        )
                        .with_diagnostics(diagnostics)
                        .with_diagnostic_model(diagnostic_model_id)
                        .with_diagnostic_feed(feed.clone())
                        .with_model_params(reasoning_params)
                        .with_turn_budget(turn_budget)
                        .with_recovery(
                            if metadata {
                                0
                            } else {
                                self.config.context.continuation_retries
                            },
                            self.config.context.continuation_retry_delay_ms,
                            feed.clone(),
                        ),
                    )
                    .preamble(preamble),
                    tools,
                    channel,
                    prompt,
                    history,
                    feed,
                )
                .await
            }
            AuthMode::Subscription => {
                let sub = if force_refresh {
                    self.auth.force_refresh().await
                } else {
                    self.auth.fresh().await
                }
                .map_err(|_| {
                    unavailable("Subscription authentication failed; check albert login.")
                })?;
                let client = self
                    .subscription_client(
                        &sub,
                        model
                            .base_url
                            .as_deref()
                            .unwrap_or(&self.config.subscription_base_url),
                        http,
                    )
                    .map_err(|_| unavailable("Could not construct subscription client."))?;
                let model = CodexResponsesModel::make(&client, &model.model);
                self.drive_attempt(
                    AgentBuilder::new(
                        BudgetedModel::new(
                            ObservedModel::new(model, preview).with_accounting(accounting),
                            budget,
                            false,
                        )
                        .with_diagnostics(diagnostics)
                        .with_diagnostic_model(diagnostic_model_id)
                        .with_diagnostic_feed(feed.clone())
                        .with_model_params(reasoning_params)
                        .with_turn_budget(turn_budget)
                        .with_recovery(
                            if metadata {
                                0
                            } else {
                                self.config.context.continuation_retries
                            },
                            self.config.context.continuation_retry_delay_ms,
                            feed.clone(),
                        ),
                    )
                    .preamble(preamble),
                    tools,
                    channel,
                    prompt,
                    history,
                    feed,
                )
                .await
            }
        };
        if matches!(&result, Err(PromptError::PromptCancelled { .. })) && control_feed.is_live() {
            if let Some(wait) = control_feed.confirmation_wait() {
                return Ok(AgentOutcome::AwaitingConfirmation(wait));
            }
        }
        let result = result.map_err(|error| {
            let failure = model_failure(error);
            warn!(model_id=%model.id, compact, timeout_ms, elapsed_ms=started.elapsed().as_millis(),
                kind=?failure.kind, "model attempt failed");
            failure
        })?;
        if result.trim().is_empty() {
            return Err(Failure {
                kind: FailureKind::Incompatible,
                message: "Model returned an empty answer.".into(),
            });
        }
        Ok(AgentOutcome::Answer(result))
    }

    /// A ChatGPT-subscription rig client: rig's OpenAI provider (Responses API by
    /// default) pointed at the Codex backend, with the OAuth access token as the
    /// bearer and the account id in the mandatory `ChatGPT-Account-ID` header.
    pub(in crate::cogitator) fn subscription_client(
        &self,
        sub: &SubToken,
        base_url: &str,
        http: ReqwestClient,
    ) -> Result<openai::Client<CodexHttp>, String> {
        if let Some(plan) = &sub.plan {
            info!(plan, "subscription auth loaded");
        }
        let mut headers = HeaderMap::new();
        headers.insert(
            "chatgpt-account-id",
            HeaderValue::from_str(&sub.account_id).map_err(|e| format!("bad account id: {e}"))?,
        );
        headers.insert("originator", HeaderValue::from_static("codex_cli_rs"));
        openai::Client::builder()
            .base_url(base_url)
            .api_key(sub.access_token.as_str())
            .http_headers(headers)
            .http_client(CodexHttp::with_client(http))
            .build()
            .map_err(|e| format!("client build: {e}"))
    }
}
