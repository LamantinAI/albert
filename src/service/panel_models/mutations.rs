use std::{collections::HashSet, fs::remove_file, path::Path};

use rand::random;
use serde_json::from_value;
use toml::{to_string, to_string_pretty, Table, Value};

use super::{
    files::{atomic_write, check_revision, checked, read_optional},
    views::Loaded,
    ModelService,
};
use crate::{
    api::{
        errors::{ApiError, ApiResult},
        host::ModelOperation,
        models::{ModelConfig, ModelPoolView, ModelWrite},
    },
    config::AuthMode,
    models::{ModelSpec, PoolConfig},
    selfconfig::SelfConfig,
};

const MAXIMUM: usize = 1024 * 1024;

impl ModelService {
    /// The owner configuration service already holds the shared write lease.
    pub(crate) fn replace_document_in(&self, content: &str) -> ApiResult<()> {
        let loaded = self.load_with_pool(Some(content))?;
        self.commit(&loaded, content.as_bytes(), None)
    }

    pub(super) fn mutate(&self, operation: ModelOperation) -> ApiResult<ModelPoolView> {
        let loaded = self.load()?;
        if matches!(operation, ModelOperation::List) {
            return self.view(&loaded);
        }
        let mut pool = loaded.pool.clone().unwrap_or(PoolConfig {
            default: String::new(),
            max_attempts: 3,
            retries_per_model: 0,
            retry_delay_ms: 2000,
            models: Vec::new(),
            roles: Vec::new(),
        });
        // Existing implicit key references retain the global key when a legacy
        // configuration is promoted into an explicit pool.
        for model in &mut pool.models {
            if model.provider == AuthMode::ApiKey && model.api_key_env.is_none() {
                model.api_key_env = Some(loaded.legacy.openai_key_env.clone());
            }
        }
        let mut secret = None;
        match operation {
            ModelOperation::Create(input) => {
                check_revision(input.revision.as_deref(), &loaded.revision)?;
                if pool.models.iter().any(|model| model.id == input.config.id) {
                    return Err(ApiError::conflict("A model with this ID already exists."));
                }
                let (model, write) = self.convert(input, None, &loaded, &pool)?;
                if pool.models.is_empty() {
                    pool.default = model.id.clone();
                }
                pool.models.push(model);
                secret = write;
            }
            ModelOperation::Update { id, input } => {
                require_revision(input.revision.as_deref(), &loaded.revision)?;
                if input.config.id != id {
                    return Err(ApiError::invalid("The model ID must match its resource."));
                }
                let index = pool
                    .models
                    .iter()
                    .position(|model| model.id == id)
                    .ok_or_else(ApiError::not_found)?;
                let (model, write) =
                    self.convert(input, Some(&pool.models[index]), &loaded, &pool)?;
                pool.models[index] = model;
                secret = write;
            }
            ModelOperation::Delete { id, revision } => {
                require_revision(revision.as_deref(), &loaded.revision)?;
                let index = pool
                    .models
                    .iter()
                    .position(|model| model.id == id)
                    .ok_or_else(ApiError::not_found)?;
                if pool.models.len() == 1 {
                    return Err(ApiError::invalid(
                        "Keep at least one configured model, or replace it first.",
                    ));
                }
                pool.models.remove(index);
                if pool.default == id {
                    pool.default = pool.models[0].id.clone();
                }
            }
            ModelOperation::Order(input) => {
                check_revision(Some(&input.revision), &loaded.revision)?;
                let ids = input.ids.iter().collect::<HashSet<_>>();
                if input.ids.len() != pool.models.len()
                    || ids.len() != input.ids.len()
                    || pool.models.iter().any(|model| !ids.contains(&model.id))
                {
                    return Err(ApiError::invalid(
                        "Model order must contain every configured ID exactly once.",
                    ));
                }
                pool.models = input
                    .ids
                    .iter()
                    .map(|id| {
                        pool.models
                            .iter()
                            .find(|model| model.id == *id)
                            .cloned()
                            .ok_or_else(ApiError::not_found)
                    })
                    .collect::<ApiResult<Vec<_>>>()?;
            }
            ModelOperation::Preference(input) => {
                pool.default = input.model_id;
            }
            ModelOperation::List => return self.view(&loaded),
        }
        pool.validate().map_err(ApiError::invalid)?;
        let text = to_string_pretty(&pool)
            .map_err(|_| ApiError::invalid("The model configuration cannot be serialized."))?;
        if text.len() > MAXIMUM {
            return Err(ApiError::invalid("The model pool exceeds its size limit."));
        }
        self.commit(&loaded, text.as_bytes(), secret)?;
        self.view(&self.load()?)
    }

    fn convert(
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

    fn unique_key(&self, id: &str, loaded: &Loaded, pool: &PoolConfig) -> String {
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

    fn commit(
        &self,
        loaded: &Loaded,
        pool: &[u8],
        secret: Option<(String, String)>,
    ) -> ApiResult<()> {
        if read_optional(&loaded.root, &loaded.base_path, MAXIMUM)? != loaded.base_bytes
            || read_optional(&loaded.root, &loaded.path, MAXIMUM)? != loaded.pool_bytes
        {
            return Err(ApiError::conflict(
                "Configuration changed outside the panel. Reload before saving.",
            ));
        }
        let env_path = checked(&loaded.root, Path::new(".env"))?;
        let old_env = read_optional(&loaded.root, &env_path, MAXIMUM)?;
        let mut written_env = None;
        if let Some((name, value)) = secret {
            SelfConfig::new(loaded.root.clone())
                .panel_set_secret(&name, &value)
                .map_err(|_| ApiError::internal())?;
            written_env = read_optional(&loaded.root, &env_path, MAXIMUM)?;
        }
        let mut written_base = None;
        let outcome = (|| {
            atomic_write(&loaded.root, &loaded.path, pool)?;
            if !loaded.document.contains_key("model_pool") {
                let relative = loaded
                    .path
                    .strip_prefix(&loaded.root)
                    .ok()
                    .and_then(|path| path.to_str())
                    .ok_or_else(ApiError::internal)?;
                let mut pointer = Table::new();
                pointer.insert("model_pool".into(), Value::String(relative.into()));
                let mut base = to_string(&pointer)
                    .map_err(|_| ApiError::internal())?
                    .into_bytes();
                base.extend_from_slice(loaded.base_bytes.as_deref().unwrap_or_default());
                if read_optional(&loaded.root, &loaded.base_path, MAXIMUM)? != loaded.base_bytes {
                    return Err(ApiError::conflict(
                        "Albert configuration changed outside the panel.",
                    ));
                }
                written_base = Some(base.clone());
                atomic_write(&loaded.root, &loaded.base_path, &base)?;
            }
            Ok(())
        })();
        if outcome.is_err() {
            if let Some(written) = written_base {
                restore(
                    &loaded.root,
                    &loaded.base_path,
                    loaded.base_bytes.as_deref(),
                    Some(&written),
                )?;
            }
            restore(
                &loaded.root,
                &loaded.path,
                loaded.pool_bytes.as_deref(),
                Some(pool),
            )?;
            if let Some(written) = written_env {
                restore(&loaded.root, &env_path, old_env.as_deref(), Some(&written))?;
            }
        }
        outcome
    }
}

fn restore(
    root: &Path,
    path: &Path,
    original: Option<&[u8]>,
    expected: Option<&[u8]>,
) -> ApiResult<()> {
    let actual = read_optional(root, path, MAXIMUM)?;
    if actual.as_deref() == original {
        return Ok(());
    }
    if actual.as_deref() != expected {
        return Err(ApiError::conflict(
            "Configuration changed during rollback; reload its state.",
        ));
    }
    match original {
        Some(bytes) => atomic_write(root, path, bytes),
        None => remove_file(checked(root, path)?).map_err(|_| ApiError::internal()),
    }
}

fn require_revision(expected: Option<&str>, actual: &str) -> ApiResult<()> {
    if expected.is_none() {
        return Err(ApiError::conflict(
            "Reload the model pool before changing this resource.",
        ));
    }
    check_revision(expected, actual)
}
