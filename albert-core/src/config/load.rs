//! Read, validate and resolve an agent configuration.
use super::{
    config_path, default_multimodal, resolve, resolve_path, schema::Raw, secrets::read_secret,
    AuthMode, CloudEndpoint, Config,
};
use crate::{error::Error, error::Result, models::PoolConfig};
use chrono_tz::{Tz, UTC};
use std::{collections::HashMap, fs::read_to_string, path::Path};
use toml::{from_str, Value};
impl Config {
    pub fn load() -> Result<Self> {
        Self::load_from(&config_path())
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        let text = read_to_string(path)
            .map_err(|_| Error::Config("Agent configuration could not be read.".into()))?;
        Self::parse_text(&text, path)
    }

    pub fn parse_text(text: &str, path: &Path) -> Result<Self> {
        let mut raw: Raw = from_str(text)?;
        let dir = path.parent().unwrap_or_else(|| Path::new("."));

        let key_var = raw.openai_key_env.as_deref().unwrap_or("ALBERT_OPENAI_KEY");
        let auth = match raw.auth.as_deref() {
            None | Some("api_key") => AuthMode::ApiKey,
            Some("subscription") => AuthMode::Subscription,
            Some(other) => {
                return Err(Error::Config(format!(
                    "unknown auth {other:?}; use \"api_key\" or \"subscription\""
                )))
            }
        };
        // The API key is required only for the api-key path; a subscription reads
        // its bearer from the OAuth token store, so the env var may be unset.
        let api_key = read_secret(dir, key_var).filter(|value| !value.is_empty());

        let timezone = raw
            .timezone
            .as_deref()
            .map(str::parse::<Tz>)
            .transpose()
            .map_err(|e| Error::Config(format!("invalid timezone: {e}")))?
            .unwrap_or(UTC);

        let multimodal = raw
            .multimodal
            .unwrap_or_else(|| default_multimodal(auth, &raw.model));

        // Cloud memory: one [clouds.<name>] table each (url + token_env), plus an
        // optional [clouds] default. Absent -> empty map -> Albert stays local-only.
        let mut clouds = HashMap::new();
        let mut clouds_default = None;
        for (name, val) in &raw.clouds {
            if name == "default" {
                clouds_default = val.as_str().map(str::to_string);
                continue;
            }
            match (
                val.get("url").and_then(Value::as_str),
                val.get("token_env").and_then(Value::as_str),
            ) {
                (Some(url), Some(token_env)) => {
                    clouds.insert(
                        name.clone(),
                        CloudEndpoint {
                            url: url.to_string(),
                            token_env: token_env.to_string(),
                        },
                    );
                }
                _ => {
                    return Err(Error::Config(format!(
                        "cloud '{name}' needs both url and token_env"
                    )))
                }
            }
        }

        raw.memory.validate(dir, !clouds.is_empty())?;
        let model_pool = raw.model_pool.as_ref().map(|p| resolve_path(dir, p));
        let models = model_pool
            .as_deref()
            .map(PoolConfig::read)
            .transpose()
            .map_err(Error::Config)?;
        raw.context.validate().map_err(Error::Config)?;
        raw.authorization.validate().map_err(Error::Config)?;
        raw.subagents.validate().map_err(Error::Config)?;
        raw.scratchpad.validate().map_err(Error::Config)?;
        raw.sessions.validate()?;
        Ok(Config {
            sessions: raw.sessions,
            authorization: raw.authorization,
            context: raw.context,
            subagents: raw.subagents,
            scratchpad: raw.scratchpad,
            model_pool,
            mcp_registry: resolve(
                dir,
                raw.mcp_registry.as_deref().unwrap_or("config/mcp.toml"),
            ),
            models,
            memory: raw.memory,
            multimodal,
            stream_status: raw.stream_status.unwrap_or(true),
            model: raw.model,
            auth,
            base_url: raw.base_url,
            api_key,
            openai_key_env: key_var.to_owned(),
            subscription_auth_json: resolve_path(dir, &raw.subscription.auth_json),
            subscription_base_url: raw.subscription.base_url,
            history: Some(raw.history.backend),
            history_backup_dir: resolve_path(dir, &raw.history.backup_dir),
            scheduler_state_path: resolve(dir, &raw.scheduler.state_path),
            reflection_secs: raw.reflection.period_secs,
            soul_path: resolve(dir, &raw.prompt.soul),
            system_path: resolve(dir, &raw.prompt.system),
            max_tool_turns: raw.agent.max_tool_turns,
            connectors_manifest: resolve(
                dir,
                raw.connectors_manifest
                    .as_deref()
                    .unwrap_or("config/octo.toml"),
            ),
            skills_dir: resolve(dir, &raw.skills.dir),
            skills_cache: raw.skills.cache,
            skills_page: raw.skills.page.max(1),
            timezone,
            clouds,
            clouds_default,
            code_workspace: resolve(dir, &raw.code.workspace),
            deploy_dir: dir.to_path_buf(),
        })
    }
}
