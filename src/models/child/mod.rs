use std::collections::HashSet;

use serde_json::{json, Value};

use super::Snapshot;
use crate::scratchpad::ResolvedAssignment;

impl Snapshot {
    pub fn allowed_child_models(&self) -> Vec<String> {
        let mut ids = vec![self.selected.clone()];
        for role in &self.config.roles {
            for id in &role.models {
                if !ids.contains(id) {
                    ids.push(id.clone());
                }
            }
        }
        ids
    }
    pub fn resolve_child(
        &self,
        role: Option<&str>,
        models: &[String],
        effort: Option<&str>,
    ) -> Result<ResolvedAssignment, String> {
        let profile = role
            .map(|id| {
                self.config
                    .roles
                    .iter()
                    .find(|r| r.id == id)
                    .ok_or_else(|| format!("Unknown child role: {id}"))
            })
            .transpose()?;
        let allowed = profile
            .map(|r| r.models.clone())
            .unwrap_or_else(|| self.allowed_child_models());
        let selected = if models.is_empty() {
            profile
                .map(|r| r.models.clone())
                .unwrap_or_else(|| vec![self.selected.clone()])
        } else {
            models.to_vec()
        };
        let resolved_effort = effort
            .map(String::from)
            .or_else(|| profile.and_then(|r| r.reasoning_effort.clone()));
        let mut seen = HashSet::new();
        for id in &selected {
            if !seen.insert(id) {
                return Err(format!("Duplicate child model: {id}"));
            }
            if !allowed.contains(id) {
                return Err(format!(
                    "Child model is outside permitted role profiles: {id}"
                ));
            }
            let model = self
                .config
                .models
                .iter()
                .find(|m| &m.id == id)
                .ok_or_else(|| format!("Unknown child model: {id}"))?;
            model.validate_effort(resolved_effort.as_deref())?;
        }
        Ok(ResolvedAssignment {
            role: role.map(String::from),
            models: selected,
            reasoning_effort: resolved_effort,
        })
    }
    pub fn child_snapshot(&self, assignment: &ResolvedAssignment) -> Result<Self, String> {
        let validated = self.resolve_child(
            assignment.role.as_deref(),
            &assignment.models,
            assignment.reasoning_effort.as_deref(),
        )?;
        let mut child = self.scoped(&validated.models)?;
        for model in &mut child.config.models {
            model.reasoning_effort = validated.reasoning_effort.clone();
        }
        Ok(child)
    }
    pub fn child_capabilities(&self) -> Value {
        let models = self
            .allowed_child_models()
            .iter()
            .filter_map(|id| self.config.models.iter().find(|m| &m.id == id))
            .map(|m| json!({"id":m.id,"tools":m.tools,"vision":m.vision,"reasoning":m.reasoning}))
            .collect::<Vec<_>>();
        json!({"default_model":self.selected,"default_models":[self.selected],"models":models,"roles":self.config.roles.iter().map(|r|json!({"id":r.id,"description":r.description,"models":r.models,"reasoning_effort":r.reasoning_effort})).collect::<Vec<_>>()})
    }
}

#[cfg(test)]
mod tests {
    use std::{
        future::ready,
        sync::{Arc, RwLock},
    };

    use serde_json::json;

    use super::Snapshot;
    use crate::{
        config::AuthMode,
        models::{
            Failure, ModelPool, ModelSpec, PoolConfig, ReasoningSupport, ReasoningWire, RoleProfile,
        },
    };

    fn spec(id: &str) -> ModelSpec {
        ModelSpec {
            id: id.into(),
            model: id.into(),
            context_window: None,
            provider: AuthMode::ApiKey,
            base_url: None,
            api_key_env: None,
            vision: true,
            tools: true,
            request_timeout_ms: 1000,
            reasoning: Some(ReasoningSupport {
                wire: ReasoningWire::ReasoningObject,
                supported_efforts: vec!["low".into(), "high".into()],
            }),
            reasoning_effort: None,
        }
    }
    fn snapshot() -> Snapshot {
        Snapshot {
            selected: "primary".into(),
            config: PoolConfig {
                default: "primary".into(),
                max_attempts: 3,
                retries_per_model: 0,
                retry_delay_ms: 0,
                models: vec![
                    spec("primary"),
                    spec("worker"),
                    spec("reviewer"),
                    spec("private"),
                ],
                roles: vec![
                    RoleProfile {
                        id: "build".into(),
                        description: "Build".into(),
                        models: vec!["worker".into(), "primary".into()],
                        reasoning_effort: Some("low".into()),
                    },
                    RoleProfile {
                        id: "review".into(),
                        description: "Review".into(),
                        models: vec!["reviewer".into()],
                        reasoning_effort: None,
                    },
                ],
            },
        }
    }
    #[test]
    fn child_defaults_only_to_snapshot_primary_and_profiles_are_opt_in() {
        let mut root = snapshot();
        root.config.roles.clear();
        let assignment = root.resolve_child(None, &[], None).unwrap();
        assert_eq!(assignment.models, ["primary"]);
        assert!(root.resolve_child(None, &["worker".into()], None).is_err());
        assert_eq!(
            root.child_snapshot(&assignment)
                .unwrap()
                .config
                .models
                .len(),
            1
        );
        assert_eq!(root.config.models.len(), 4);
        let root = snapshot();
        assert_eq!(
            root.allowed_child_models(),
            ["primary", "worker", "reviewer"]
        );
        let default = root.resolve_child(None, &[], None).unwrap();
        assert_eq!(default.models, ["primary"]);
        assert_eq!(
            root.child_capabilities()["models"]
                .as_array()
                .unwrap()
                .iter()
                .map(|m| m["id"].clone())
                .collect::<Vec<_>>(),
            vec![json!("primary"), json!("worker"), json!("reviewer")]
        );
        assert!(root.resolve_child(None, &["private".into()], None).is_err());
    }
    #[test]
    fn role_order_subset_effort_and_unknowns_are_explicit() {
        let root = snapshot();
        let role = root.resolve_child(Some("build"), &[], None).unwrap();
        assert_eq!(role.models, ["worker", "primary"]);
        assert_eq!(role.reasoning_effort.as_deref(), Some("low"));
        let selected = root
            .resolve_child(Some("build"), &["primary".into()], Some("high"))
            .unwrap();
        assert_eq!(selected.models, ["primary"]);
        assert_eq!(selected.reasoning_effort.as_deref(), Some("high"));
        let child = root.child_snapshot(&selected).unwrap();
        assert_eq!(
            child.config.models[0].reasoning_effort.as_deref(),
            Some("high")
        );
        assert!(root.resolve_child(Some("missing"), &[], None).is_err());
        assert!(root
            .resolve_child(Some("build"), &["reviewer".into()], None)
            .is_err());
        assert!(root
            .resolve_child(None, &["worker".into(), "worker".into()], None)
            .is_err());
        assert!(root.resolve_child(None, &["missing".into()], None).is_err());
        assert!(root.resolve_child(None, &[], Some("ultra")).is_err());
        let mut unsupported = root.clone();
        unsupported.config.models[0].reasoning = None;
        assert!(unsupported.resolve_child(Some("build"), &[], None).is_err());
    }
    #[test]
    fn resolution_and_scoping_keep_the_same_frozen_selection() {
        let pool = ModelPool {
            state: Arc::new(RwLock::new(snapshot())),
            path: None,
        };
        let old = pool.snapshot();
        pool.command("worker", true);
        let old_assignment = old.resolve_child(None, &[], None).unwrap();
        assert_eq!(old_assignment.models, ["primary"]);
        assert_eq!(
            old.child_snapshot(&old_assignment).unwrap().selected,
            "primary"
        );
        assert_eq!(
            pool.snapshot()
                .resolve_child(None, &[], None)
                .unwrap()
                .models,
            ["worker"]
        );
    }
    #[tokio::test]
    async fn unsupported_effort_never_enters_provider_callback() {
        let mut snapshot = snapshot();
        snapshot.config.models.truncate(1);
        snapshot.config.models[0].reasoning_effort = Some("unknown".into());
        let error = snapshot
            .run(
                false,
                |_, _| {
                    panic!("unsupported effort invoked provider");
                    #[allow(unreachable_code)]
                    ready(Ok::<(), Failure>(()))
                },
                || false,
                |_| ready(()),
            )
            .await
            .unwrap_err();
        assert!(error.contains("0/3 attempts"));
        assert!(error.contains("does not declare reasoning effort"));
    }
}
