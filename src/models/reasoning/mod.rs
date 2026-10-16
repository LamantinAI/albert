//! Explicit, provider-declared effort support; never infer capabilities from names.
use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{config::AuthMode, models::ModelSpec};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningWire {
    ReasoningObject,
    ReasoningEffort,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReasoningSupport {
    pub wire: ReasoningWire,
    pub supported_efforts: Vec<String>,
}
impl ModelSpec {
    pub fn validate_reasoning(&self) -> Result<(), String> {
        if let Some(support) = &self.reasoning {
            if self.provider == AuthMode::Subscription
                && support.wire != ReasoningWire::ReasoningObject
            {
                return Err(format!(
                    "Model {} subscription requires reasoning_object wire",
                    self.id
                ));
            }
            let mut efforts = HashSet::new();
            if support.supported_efforts.is_empty()
                || support
                    .supported_efforts
                    .iter()
                    .any(|e| e.trim().is_empty() || e.trim() != e || !efforts.insert(e))
            {
                return Err(format!(
                    "Model {} needs unique nonempty supported efforts",
                    self.id
                ));
            }
        }
        self.validate_effort(self.reasoning_effort.as_deref())
    }
    pub fn validate_effort(&self, effort: Option<&str>) -> Result<(), String> {
        if let Some(effort) = effort {
            if !self
                .reasoning
                .as_ref()
                .is_some_and(|r| r.supported_efforts.iter().any(|e| e == effort))
            {
                return Err(format!(
                    "Model {} does not declare reasoning effort {effort}",
                    self.id
                ));
            }
            if self.provider == AuthMode::Subscription
                && self
                    .reasoning
                    .as_ref()
                    .is_some_and(|r| r.wire != ReasoningWire::ReasoningObject)
            {
                return Err(format!(
                    "Model {} subscription requires reasoning_object wire",
                    self.id
                ));
            }
        }
        Ok(())
    }
    pub fn reasoning_params(&self) -> Result<Option<Value>, String> {
        self.validate_reasoning()?;
        let Some(effort) = &self.reasoning_effort else {
            return Ok(None);
        };
        let support = self.reasoning.as_ref().ok_or("reasoning support missing")?;
        Ok(Some(match support.wire {
            ReasoningWire::ReasoningObject => json!({"reasoning":{"effort":effort}}),
            ReasoningWire::ReasoningEffort => json!({"reasoning_effort":effort}),
        }))
    }
}

#[cfg(test)]
mod fixture;
#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rig::completion::{CompletionModel, Message};
    use serde_json::json;
    use tokio::time::timeout;

    use super::{fixture::Server, ReasoningSupport, ReasoningWire};
    use crate::{
        config::AuthMode,
        context::BudgetedModel,
        models::{ModelSpec, PoolConfig},
        status::StatusFeed,
    };

    fn spec(provider: AuthMode, wire: ReasoningWire, effort: Option<&str>) -> ModelSpec {
        ModelSpec {
            id: "fixture".into(),
            model: "fixture-model".into(),
            context_window: None,
            provider,
            base_url: None,
            api_key_env: None,
            vision: true,
            tools: true,
            request_timeout_ms: 2000,
            reasoning: Some(ReasoningSupport {
                wire,
                supported_efforts: vec!["high".into(), "low".into()],
            }),
            reasoning_effort: effort.map(String::from),
        }
    }
    #[tokio::test]
    async fn api_both_explicit_shapes_preserve_other_parameters_every_round() {
        for wire in [
            ReasoningWire::ReasoningObject,
            ReasoningWire::ReasoningEffort,
        ] {
            let server = Server::new().await;
            let spec = spec(AuthMode::ApiKey, wire, Some("high"));
            let model = BudgetedModel::new(server.api(), None, true)
                .with_model_params(spec.reasoning_params().unwrap());
            for _ in 0..2 {
                let mut request = model.completion_request(Message::user("continue")).build();
                request.max_tokens = Some(77);
                request.additional_params = Some(json!({"temperature":0.4,"fixture":"retained"}));
                timeout(Duration::from_secs(3), model.completion(request))
                    .await
                    .unwrap()
                    .unwrap();
            }
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            for request in requests.iter() {
                assert_eq!(request["max_tokens"], 77);
                assert_eq!(request["temperature"], 0.4);
                assert_eq!(request["fixture"], "retained");
                match wire {
                    ReasoningWire::ReasoningObject => {
                        assert_eq!(request["reasoning"]["effort"], "high");
                        assert!(request.get("reasoning_effort").is_none());
                    }
                    ReasoningWire::ReasoningEffort => {
                        assert_eq!(request["reasoning_effort"], "high");
                        assert!(request.get("reasoning").is_none());
                    }
                }
            }
        }
    }
    #[tokio::test]
    async fn subscription_responses_applies_declared_object_on_each_round() {
        let server = Server::new().await;
        let spec = spec(
            AuthMode::Subscription,
            ReasoningWire::ReasoningObject,
            Some("high"),
        );
        let model = BudgetedModel::new(server.subscription(), None, false)
            .with_model_params(spec.reasoning_params().unwrap());
        for _ in 0..2 {
            let mut request = model.completion_request(Message::user("continue")).build();
            request.max_tokens = Some(77);
            timeout(Duration::from_secs(3), model.completion(request))
                .await
                .unwrap()
                .unwrap();
        }
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        for request in requests.iter() {
            assert_eq!(request["reasoning"]["effort"], "high");
            assert_eq!(request["stream"], true);
            assert!(request.get("reasoning_effort").is_none());
            assert!(request.get("max_tokens").is_none());
            assert!(request.get("max_output_tokens").is_none());
        }
    }
    #[tokio::test]
    async fn no_explicit_effort_leaves_both_provider_payloads_unchanged() {
        let server = Server::new().await;
        let model = BudgetedModel::new(server.api(), None, true).with_model_params(
            spec(AuthMode::ApiKey, ReasoningWire::ReasoningEffort, None)
                .reasoning_params()
                .unwrap(),
        );
        timeout(
            Duration::from_secs(3),
            model.completion(model.completion_request(Message::user("hello")).build()),
        )
        .await
        .unwrap()
        .unwrap();
        let model = BudgetedModel::new(server.subscription(), None, false);
        timeout(
            Duration::from_secs(3),
            model.completion(model.completion_request(Message::user("hello")).build()),
        )
        .await
        .unwrap()
        .unwrap();
        for request in server.requests.lock().unwrap().iter() {
            assert!(request.get("reasoning").is_none());
            assert!(request.get("reasoning_effort").is_none());
        }
    }
    #[tokio::test]
    async fn continuation_retries_keep_effort_and_budget_parameters() {
        let server = Server::new().await;
        let spec = spec(
            AuthMode::ApiKey,
            ReasoningWire::ReasoningObject,
            Some("low"),
        );
        let model = BudgetedModel::new(server.api(), None, true)
            .with_model_params(spec.reasoning_params().unwrap())
            .with_recovery(1, 0, StatusFeed::silent());
        let mut request = model
            .completion_request(Message::tool_result("call_1", "result"))
            .build();
        request.max_tokens = Some(55);
        request.additional_params = Some(json!({"retry_fixture":true}));
        timeout(Duration::from_secs(3), model.completion(request))
            .await
            .unwrap()
            .unwrap();
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        for request in requests.iter() {
            assert_eq!(request["reasoning"]["effort"], "low");
            assert_eq!(request["max_tokens"], 55);
        }
    }
    #[test]
    fn declarations_validate_exact_efforts_roles_and_subscription_wire() {
        assert!(
            spec(AuthMode::Subscription, ReasoningWire::ReasoningEffort, None)
                .validate_reasoning()
                .is_err()
        );
        assert!(spec(
            AuthMode::ApiKey,
            ReasoningWire::ReasoningObject,
            Some("HIGH")
        )
        .reasoning_params()
        .is_err());
        let base="default='p'\n[[models]]\nid='p'\nmodel='test'\nprovider='api_key'\nvision=true\ntools=true\nreasoning={wire='reasoning_object',supported_efforts=['high']}\n";
        assert!(PoolConfig::parse(base).is_ok());
        assert!(PoolConfig::parse(&format!("{base}[[roles]]\nid='review'\ndescription='Review'\nmodels=['p']\nreasoning_effort='high'\n")).is_ok());
        for suffix in [
            "[[roles]]\nid='x'\ndescription='x'\nmodels=['missing']",
            "[[roles]]\nid='x'\ndescription='x'\nmodels=['p','p']",
            "[[roles]]\nid='x'\ndescription='x'\nmodels=['p']\nreasoning_effort='low'",
        ] {
            assert!(PoolConfig::parse(&format!("{base}{suffix}")).is_err());
        }
    }
}
