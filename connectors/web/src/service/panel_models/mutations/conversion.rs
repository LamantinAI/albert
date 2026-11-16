use crate::service::panel_models::{views::Loaded, ModelService};
use crate::{
    api::errors::ApiError, api::errors::ApiResult, api::models::ModelConfig,
    api::models::ModelWrite,
};
use albert_core::{
    config::AuthMode, models::ModelSpec, models::PoolConfig, selfconfig::SelfConfig,
};
use rand::random;
use serde_json::from_value;

impl ModelService {
    pub(in crate::service::panel_models) fn convert(
        &self,
        input: ModelWrite,
        old: Option<&ModelSpec>,
        loaded: &Loaded,
        pool: &PoolConfig,
    ) -> ApiResult<(ModelSpec, Option<(String, String)>)> {
        let ModelConfig {
            id,
            model,
            provider,
            base_url,
            api_key_env,
            context_window,
            vision,
            tools,
            reasoning,
            request_timeout_ms,
        } = input.config;
        if id.len() > 4096
            || model.len() > 4096
            || base_url.as_ref().is_some_and(|url| url.len() > 4096)
        {
            return Err(ApiError::invalid("Model settings exceed their size limit."));
        }
        let provider = match provider.as_str() {
            "api_key" => AuthMode::ApiKey,
            "subscription" => AuthMode::Subscription,
            _ => {
                return Err(ApiError::invalid(
                    "Provider must be api_key or subscription.",
                ))
            }
        };
        if provider == AuthMode::Subscription && input.api_key.is_some() {
            return Err(ApiError::invalid(
                "Subscription models use the host-managed subscription credential.",
            ));
        }
        let reference = if provider == AuthMode::ApiKey {
            let reference = api_key_env
                .or_else(|| old.and_then(|model| model.api_key_env.clone()))
                .unwrap_or_else(|| self.unique_key(&id, loaded, pool));
            if reference.is_empty()
                || reference.len() > 128
                || !reference.chars().enumerate().all(|(index, c)| {
                    c.is_ascii_alphabetic() || c == '_' || (index > 0 && c.is_ascii_digit())
                })
            {
                return Err(ApiError::invalid(
                    "The credential reference must be a valid environment variable name.",
                ));
            }
            Some(reference)
        } else {
            None
        };
        let secret = match input.api_key {
            Some(value)
                if value.is_empty()
                    || value.len() > 64 * 1024
                    || value.chars().any(|c| matches!(c, '\n' | '\r' | '\0')) =>
            {
                return Err(ApiError::invalid(
                    "The API key is empty, too large or contains a forbidden character.",
                ))
            }
            Some(value) => Some((reference.clone().ok_or_else(ApiError::internal)?, value)),
            None => None,
        };
        Ok((
            ModelSpec {
                id,
                model,
                provider,
                base_url,
                api_key_env: reference,
                context_window,
                vision,
                tools,
                reasoning: reasoning
                    .filter(|value| !value.is_null())
                    .map(from_value)
                    .transpose()
                    .map_err(|_| ApiError::invalid("Invalid reasoning capability declaration."))?,
                reasoning_effort: None,
                request_timeout_ms: request_timeout_ms
                    .or_else(|| old.map(|model| model.request_timeout_ms))
                    .unwrap_or(120_000),
            },
            secret,
        ))
    }
    pub(in crate::service::panel_models) fn unique_key(
        &self,
        id: &str,
        loaded: &Loaded,
        pool: &PoolConfig,
    ) -> String {
        let existing = SelfConfig::new(loaded.root.clone()).panel_secret_names();
        loop {
            let name = format!(
                "ALBERT_MODEL_{}_{:016X}_KEY",
                id.chars()
                    .take(48)
                    .collect::<String>()
                    .to_ascii_uppercase()
                    .replace('-', "_"),
                random::<u64>()
            );
            if !existing.contains(&name)
                && !pool
                    .models
                    .iter()
                    .any(|model| model.api_key_env.as_ref() == Some(&name))
            {
                return name;
            }
        }
    }
}
