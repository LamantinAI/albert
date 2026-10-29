use std::{
    path::{Path, PathBuf},
    str::from_utf8,
};

use serde_json::to_value;
use toml::{from_str, Table};

use super::{
    files::{checked, hash, read_optional, root},
    ModelService,
};
use crate::{
    api::{
        errors::{ApiError, ApiResult},
        models::{ModelConfig, ModelPoolView, ModelRole, ModelView},
    },
    config::{AuthMode, Config},
    models::PoolConfig,
};

const MAXIMUM: usize = 1024 * 1024;

pub(super) struct Loaded {
    pub root: PathBuf,
    pub path: PathBuf,
    pub base_path: PathBuf,
    pub base_bytes: Option<Vec<u8>>,
    pub pool_bytes: Option<Vec<u8>>,
    pub document: Table,
    pub pool: Option<PoolConfig>,
    pub revision: String,
    pub legacy: Config,
}

pub(super) struct Source {
    root: PathBuf,
    pub(super) path: PathBuf,
    base_path: PathBuf,
    base_bytes: Option<Vec<u8>>,
    document: Table,
}

impl ModelService {
    pub(super) fn source(&self) -> ApiResult<Source> {
        let directory = self.config_path.parent().ok_or_else(ApiError::internal)?;
        let root = root(directory)?;
        let base_path = checked(
            &root,
            Path::new(
                self.config_path
                    .file_name()
                    .ok_or_else(ApiError::internal)?,
            ),
        )?;
        let base_bytes = read_optional(&root, &base_path, MAXIMUM)?;
        let text = base_bytes
            .as_deref()
            .map(|bytes| {
                from_utf8(bytes)
                    .map_err(|_| ApiError::invalid("Invalid Albert configuration encoding."))
            })
            .transpose()?
            .unwrap_or("");
        let document: Table =
            from_str(text).map_err(|_| ApiError::invalid("Invalid Albert configuration TOML."))?;
        let configured_path = match document.get("model_pool") {
            Some(value) => value
                .as_str()
                .ok_or_else(|| ApiError::invalid("model_pool must be a file path."))?,
            None => "config/models.toml",
        };
        let path = checked(&root, Path::new(configured_path))?;
        if path == base_path || path == root.join(".env") {
            return Err(ApiError::invalid(
                "The model pool needs its own configuration file.",
            ));
        }
        Ok(Source {
            root,
            path,
            base_path,
            base_bytes,
            document,
        })
    }

    pub(super) fn load(&self) -> ApiResult<Loaded> {
        self.load_with_pool(None)
    }

    pub(super) fn load_with_pool(&self, replacement: Option<&str>) -> ApiResult<Loaded> {
        let Source {
            root,
            path,
            base_path,
            base_bytes,
            document,
        } = self.source()?;
        let pool_bytes = read_optional(&root, &path, MAXIMUM)?;
        let mut legacy = self.base.clone();
        legacy.deploy_dir = root.clone();
        legacy.auth = match document.get("auth").and_then(|value| value.as_str()) {
            Some("subscription") => AuthMode::Subscription,
            Some("api_key") => AuthMode::ApiKey,
            None => self.base.auth,
            _ => {
                return Err(ApiError::invalid(
                    "Unsupported model provider in Albert configuration.",
                ))
            }
        };
        if let Some(model) = document.get("model").and_then(|value| value.as_str()) {
            legacy.model = model.into();
        }
        if let Some(url) = document.get("base_url").and_then(|value| value.as_str()) {
            legacy.base_url = url.into();
        }
        if let Some(name) = document
            .get("openai_key_env")
            .and_then(|value| value.as_str())
        {
            legacy.openai_key_env = name.into();
        }
        if let Some(vision) = document.get("multimodal").and_then(|value| value.as_bool()) {
            legacy.multimodal = vision;
        }
        checked(&root, Path::new(".env"))?;
        let pool = if let Some(text) = replacement {
            Some(PoolConfig::parse(text).map_err(ApiError::invalid)?)
        } else if let Some(bytes) = &pool_bytes {
            Some(
                PoolConfig::parse(
                    from_utf8(bytes)
                        .map_err(|_| ApiError::invalid("Invalid model pool encoding."))?,
                )
                .map_err(ApiError::invalid)?,
            )
        } else if document.contains_key("model_pool") {
            return Err(ApiError::invalid(
                "The configured model pool file is missing.",
            ));
        } else if document.contains_key("model")
            || legacy.auth == AuthMode::Subscription
            || legacy.secret(&legacy.openai_key_env).is_some()
        {
            let mut pool = PoolConfig::legacy(&legacy);
            let model = &mut pool.models[0];
            model.base_url = Some(if legacy.auth == AuthMode::Subscription {
                legacy.subscription_base_url.clone()
            } else {
                legacy.base_url.clone()
            });
            if legacy.auth == AuthMode::ApiKey {
                model.api_key_env = Some(legacy.openai_key_env.clone());
            }
            pool.validate().map_err(ApiError::invalid)?;
            Some(pool)
        } else {
            None
        };
        let revision = hash(&[
            base_bytes.as_deref().unwrap_or_default(),
            path.strip_prefix(&root)
                .map_err(|_| ApiError::internal())?
                .as_os_str()
                .as_encoded_bytes(),
            pool_bytes.as_deref().unwrap_or_default(),
        ]);
        Ok(Loaded {
            root,
            path,
            base_path,
            base_bytes,
            pool_bytes,
            document,
            pool,
            revision,
            legacy,
        })
    }

    pub(super) fn view(&self, loaded: &Loaded) -> ApiResult<ModelPoolView> {
        let Some(pool) = &loaded.pool else {
            return Ok(ModelPoolView {
                revision: loaded.revision.clone(),
                preferred: None,
                models: Vec::new(),
                roles: Vec::new(),
            });
        };
        let models = pool
            .models
            .iter()
            .enumerate()
            .map(|(priority, model)| {
                let api_key_env = if model.provider == AuthMode::ApiKey {
                    Some(
                        model
                            .api_key_env
                            .clone()
                            .unwrap_or_else(|| loaded.legacy.openai_key_env.clone()),
                    )
                } else {
                    None
                };
                let credential_configured = match model.provider {
                    AuthMode::ApiKey => api_key_env
                        .as_ref()
                        .is_some_and(|name| loaded.legacy.secret(name).is_some()),
                    AuthMode::Subscription => loaded.legacy.subscription_auth_json.is_file(),
                };
                Ok(ModelView {
                    config: ModelConfig {
                        id: model.id.clone(),
                        model: model.model.clone(),
                        provider: match model.provider {
                            AuthMode::ApiKey => "api_key",
                            AuthMode::Subscription => "subscription",
                        }
                        .into(),
                        base_url: model.base_url.clone(),
                        api_key_env,
                        context_window: model.context_window,
                        vision: model.vision,
                        tools: model.tools,
                        reasoning: model
                            .reasoning
                            .as_ref()
                            .map(to_value)
                            .transpose()
                            .map_err(|_| ApiError::internal())?,
                        request_timeout_ms: Some(model.request_timeout_ms),
                    },
                    credential_configured,
                    priority,
                })
            })
            .collect::<ApiResult<Vec<_>>>()?;
        Ok(ModelPoolView {
            revision: loaded.revision.clone(),
            preferred: Some(pool.default.clone()),
            models,
            roles: pool
                .roles
                .iter()
                .map(|role| ModelRole {
                    id: role.id.clone(),
                    description: role.description.clone(),
                    models: role.models.clone(),
                    reasoning_effort: role.reasoning_effort.clone(),
                })
                .collect(),
        })
    }
}
