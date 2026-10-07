use std::{collections::HashSet, fs::read_to_string, path::Path};

use mcp_http::Url;
use serde::Deserialize;
use toml::from_str;

use crate::config::{AuthMode, Config};

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelSpec {
    pub id: String,
    pub model: String,
    /// Explicit provider context limit; absent uses Albert context.window_tokens.
    #[serde(default)]
    pub context_window: Option<usize>,
    pub provider: AuthMode,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    /// Explicit capabilities: never infer a configured pool member's abilities by name.
    pub vision: bool,
    pub tools: bool,
    #[serde(default = "default_request_timeout")]
    pub request_timeout_ms: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PoolConfig {
    pub default: String,
    #[serde(default = "default_attempts")]
    pub max_attempts: usize,
    #[serde(default)]
    pub retries_per_model: usize,
    #[serde(default = "default_delay")]
    pub retry_delay_ms: u64,
    pub models: Vec<ModelSpec>,
}

fn default_request_timeout() -> u64 {
    120_000
}

fn default_attempts() -> usize {
    3
}
fn default_delay() -> u64 {
    2000
}

impl PoolConfig {
    pub fn read(path: &Path) -> Result<Self, String> {
        let text = read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let config: Self = from_str(text)
            .map_err(|_| "Invalid model pool TOML; check the configuration file.".to_string())?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), String> {
        if !(1..=32).contains(&self.max_attempts)
            || self.retries_per_model > 8
            || self.retry_delay_ms > 60_000
        {
            return Err("Model pool limits: max_attempts 1..32, retries_per_model 0..8, retry_delay_ms 0..60000.".into());
        }
        if self.models.is_empty() || self.models.len() > 32 {
            return Err("Model pool must contain 1..32 members.".into());
        }
        let mut ids = HashSet::new();
        for model in &self.models {
            if model.context_window == Some(0) {
                return Err("context_window must be positive".into());
            }
            if !(1..=1_800_000).contains(&model.request_timeout_ms) {
                return Err(format!(
                    "Model {} request_timeout_ms must be 1..1800000",
                    model.id
                ));
            }
            if model.id.is_empty()
                || !model
                    .id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
            {
                return Err("Model IDs must contain only letters, digits, '-' or '_'.".into());
            }
            if matches!(model.id.as_str(), "list" | "reload" | "status")
                || !ids.insert(model.id.as_str())
            {
                return Err(format!("Reserved or duplicate model ID: {}", model.id));
            }
            if model.model.trim().is_empty()
                || model
                    .api_key_env
                    .as_ref()
                    .is_some_and(|s| s.trim().is_empty())
            {
                return Err(format!(
                    "Empty model or key environment name for {}",
                    model.id
                ));
            }
            if let Some(base) = &model.base_url {
                let valid = Url::parse(base).ok().is_some_and(|url| {
                    matches!(url.scheme(), "https" | "http")
                        && url.host_str().is_some()
                        && url.username().is_empty()
                        && url.password().is_none()
                        && url.query().is_none()
                        && url.fragment().is_none()
                });
                if !valid {
                    return Err(format!(
                        "Model {} needs an http(s) base_url without credentials, query or fragment",
                        model.id
                    ));
                }
            }
        }
        if !ids.contains(self.default.as_str()) {
            return Err("The default model must name a configured pool member.".into());
        }
        Ok(())
    }

    pub fn legacy(config: &Config) -> Self {
        Self {
            default: "default".into(),
            max_attempts: 4,
            retries_per_model: 2,
            retry_delay_ms: 2000,
            models: vec![ModelSpec {
                id: "default".into(),
                model: config.model.clone(),
                context_window: None,
                provider: config.auth,
                base_url: None,
                api_key_env: None,
                vision: config.multimodal,
                tools: true,
                request_timeout_ms: default_request_timeout(),
            }],
        }
    }
}
