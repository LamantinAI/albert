//! Explicit subscription selection preserves unrelated model entries/preferences.
use std::sync::Arc;

use albert_core::config::AuthMode;
use serde_json::json;

use super::{files::serialized, ModelService};
use crate::{
    api::errors::ApiResult, api::host::ModelOperation, api::models::ModelConfig,
    api::models::ModelPreference, api::models::ModelWrite, api::models::SubscriptionCatalogModel,
};

impl ModelService {
    pub async fn connect_subscription(
        &self,
        model: SubscriptionCatalogModel,
        base_url: String,
        prefer: bool,
    ) -> ApiResult<(String, Option<String>)> {
        let service = self.clone();
        serialized(Arc::clone(&self.write_gate), move || {
            let loaded = service.load()?;
            let existing = loaded.pool.as_ref().and_then(|pool| pool.models.iter()
                .find(|entry| entry.provider == AuthMode::Subscription && entry.model == model.slug)).map(|entry| entry.id.clone());
            let id = if let Some(id) = existing { id } else {
                let mut id = "openai-subscription".to_owned(); let mut suffix = 2;
                while loaded.pool.as_ref().is_some_and(|pool| pool.models.iter().any(|entry| entry.id == id)) {
                    id = format!("openai-subscription-{suffix}"); suffix += 1;
                }
                service.mutate(ModelOperation::Create(ModelWrite { config: ModelConfig {
                    id: id.clone(), model: model.slug, provider: "subscription".into(), base_url: Some(base_url), api_key_env: None,
                    context_window: model.context_window, vision: model.input_modalities.iter().any(|mode| mode == "image"),
                    // Codex direct/legacy tool mode supports the normal function-call
                    // protocol. This is compatibility mapping, not a catalog badge.
                    tools: true,
                    reasoning: (!model.reasoning_efforts.is_empty()).then(|| json!({"wire":"reasoning_object","supported_efforts":model.reasoning_efforts})),
                    request_timeout_ms: None,
                }, api_key: None, revision: Some(loaded.revision) }))?;
                id
            };
            let view = if prefer { service.mutate(ModelOperation::Preference(ModelPreference { model_id: id.clone() }))? }
                else { service.view(&service.load()?)? };
            Ok((id, view.preferred))
        }).await
    }
}
