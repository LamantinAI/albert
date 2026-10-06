//! Cogitator model selection. A live change affects new turns; each running loop
//! holds an immutable snapshot, including fallback order and retry limits.
mod config;
mod run;

use std::{path::PathBuf, sync::RwLock};

use rig::{
    completion::Message,
    message::{ToolResultContent, UserContent},
};

use crate::config::Config;
pub use config::{ModelSpec, PoolConfig};
pub use run::{Failure, FailureKind};

#[derive(Clone)]
pub struct Snapshot {
    pub config: PoolConfig,
    pub selected: String,
}

impl Snapshot {
    fn ordered(&self) -> Vec<ModelSpec> {
        self.config
            .models
            .iter()
            .filter(|m| m.id == self.selected)
            .chain(self.config.models.iter().filter(|m| m.id != self.selected))
            .cloned()
            .collect()
    }
}

pub struct ModelPool {
    state: RwLock<Snapshot>,
    path: Option<PathBuf>,
}

impl ModelPool {
    pub fn new(config: &Config) -> Self {
        let pool = config
            .models
            .clone()
            .unwrap_or_else(|| PoolConfig::legacy(config));
        Self {
            state: RwLock::new(Snapshot {
                selected: pool.default.clone(),
                config: pool,
            }),
            path: config.model_pool.clone(),
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        self.state.read().unwrap().clone()
    }

    /// Only a deterministic owner reflex calls this; model tools cannot select models.
    pub fn command(&self, args: &str, owner: bool) -> String {
        if !owner {
            return "Only the owner can manage the model pool.".into();
        }
        match args.trim() {
            "" | "status" | "list" => {
                let state = self.snapshot();
                let list = state
                    .config
                    .models
                    .iter()
                    .map(|m| {
                        format!(
                            "{}{}: {} ({:?}, vision={}, tools={})",
                            if state.selected == m.id { "* " } else { "  " },
                            m.id,
                            m.model,
                            m.provider,
                            m.vision,
                            m.tools
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("Preferred model: {}\n{list}\n/model <id> selects for new turns; /model reload reloads the pool.", state.selected)
            }
            "reload" => {
                let Some(path) = &self.path else {
                    return "No model_pool file is configured.".into();
                };
                let pool = match PoolConfig::read(path) {
                    Ok(p) => p,
                    Err(e) => return format!("Pool unchanged: {e}"),
                };
                let mut state = self.state.write().unwrap();
                let selected = if pool.models.iter().any(|m| m.id == state.selected) {
                    state.selected.clone()
                } else {
                    pool.default.clone()
                };
                *state = Snapshot {
                    config: pool,
                    selected,
                };
                format!(
                    "Model pool reloaded. Preferred model: {}. Running turns keep their snapshot.",
                    state.selected
                )
            }
            id => {
                let mut state = self.state.write().unwrap();
                if !state.config.models.iter().any(|m| m.id == id) {
                    return format!("Unknown model ID: {id}. Use /model list.");
                }
                state.selected = id.into();
                format!("Preferred model: {id}. Applies to new model turns; running work continues unchanged.")
            }
        }
    }
}

pub fn needs_vision<'a>(messages: impl IntoIterator<Item = &'a Message>) -> bool {
    messages.into_iter().any(|m| match m {
        Message::User { content } => content.iter().any(|c| match c {
            UserContent::Image(_) => true,
            UserContent::ToolResult(r) => r
                .content
                .iter()
                .any(|c| matches!(c, ToolResultContent::Image(_))),
            _ => false,
        }),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        fs::write,
        future::ready,
    };

    use rig::{message::UserContent, OneOrMany};
    use tempfile::tempdir;

    use super::*;
    use crate::config::AuthMode;

    fn spec(id: &str, vision: bool, tools: bool) -> ModelSpec {
        ModelSpec {
            id: id.into(),
            model: id.into(),
            provider: AuthMode::ApiKey,
            base_url: None,
            api_key_env: None,
            vision,
            tools,
            request_timeout_ms: 120_000,
        }
    }
    fn pool() -> Snapshot {
        Snapshot {
            selected: "first".into(),
            config: PoolConfig {
                default: "first".into(),
                max_attempts: 3,
                retries_per_model: 0,
                retry_delay_ms: 0,
                models: vec![spec("first", false, true), spec("second", true, true)],
            },
        }
    }
    fn failure(kind: FailureKind) -> Result<String, Failure> {
        Err(Failure {
            kind,
            message: "safe failure".into(),
        })
    }

    #[tokio::test]
    async fn provider_failure_and_capability_mismatch_use_fallback_in_order() {
        let snapshot = pool();
        let seen = RefCell::new(vec![]);
        let answer = snapshot
            .run(
                false,
                |model, _| {
                    seen.borrow_mut().push(model.id.clone());
                    ready(if model.id == "first" {
                        failure(FailureKind::Transient)
                    } else {
                        Ok("answer".into())
                    })
                },
                || false,
                |_| ready(()),
            )
            .await
            .unwrap();
        assert_eq!(answer, "answer");
        assert_eq!(*seen.borrow(), ["first", "second"]);
        seen.borrow_mut().clear();
        snapshot
            .run(
                true,
                |model, _| {
                    seen.borrow_mut().push(model.id);
                    ready(Ok::<_, Failure>(()))
                },
                || false,
                |_| ready(()),
            )
            .await
            .unwrap();
        assert_eq!(*seen.borrow(), ["second"]);
    }

    #[tokio::test]
    async fn tool_activity_stops_fallback_and_fatal_errors_are_not_retried() {
        for kind in [FailureKind::Transient, FailureKind::Fatal] {
            let calls = Cell::new(0);
            let result = pool()
                .run(
                    false,
                    |_, _| {
                        calls.set(calls.get() + 1);
                        ready(failure(kind))
                    },
                    || kind == FailureKind::Transient,
                    |_| ready(()),
                )
                .await
                .unwrap_err();
            assert_eq!(calls.get(), 1);
            if kind == FailureKind::Transient {
                assert!(result.contains("tools may already have run"));
            }
        }
    }

    #[tokio::test]
    async fn retries_and_token_refresh_share_one_bounded_budget() {
        let mut snapshot = pool();
        snapshot.config.max_attempts = 2;
        snapshot.config.retries_per_model = 8;
        let calls = Cell::new(0);
        let error = snapshot
            .run(
                false,
                |_, _| {
                    calls.set(calls.get() + 1);
                    ready(failure(FailureKind::Transient))
                },
                || false,
                |_| ready(()),
            )
            .await
            .unwrap_err();
        assert_eq!(calls.get(), 2);
        assert!(error.contains("2/2 attempts"));
        snapshot.config.models[0].provider = AuthMode::Subscription;
        snapshot.config.max_attempts = 3;
        let seen = RefCell::new(vec![]);
        snapshot
            .run(
                false,
                |model, refresh| {
                    seen.borrow_mut().push((model.id.clone(), refresh));
                    ready(if model.id == "first" {
                        failure(FailureKind::Authentication)
                    } else {
                        Ok("fallback".into())
                    })
                },
                || false,
                |_| ready(()),
            )
            .await
            .unwrap();
        assert_eq!(
            *seen.borrow(),
            [
                ("first".into(), false),
                ("first".into(), true),
                ("second".into(), false)
            ]
        );
    }

    #[tokio::test]
    async fn no_eligible_model_never_calls_a_provider() {
        let mut snapshot = pool();
        for model in &mut snapshot.config.models {
            model.tools = false;
        }
        let error = snapshot
            .run(
                false,
                |_, _| {
                    panic!("incompatible model invoked");
                    #[allow(unreachable_code)]
                    ready(Ok::<_, Failure>(()))
                },
                || false,
                |_| ready(()),
            )
            .await
            .unwrap_err();
        assert!(error.contains("No eligible model"));
        assert!(error.contains("0/3 attempts"));
    }

    #[test]
    fn switching_and_reload_are_owner_only_atomic_and_leave_snapshots_unchanged() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("models.toml");
        let pool = ModelPool {
            state: RwLock::new(pool()),
            path: Some(path.clone()),
        };
        let old = pool.snapshot();
        assert!(pool.command("second", false).contains("Only the owner"));
        assert_eq!(pool.snapshot().selected, "first");
        pool.command("second", true);
        assert_eq!(pool.snapshot().selected, "second");
        assert_eq!(old.selected, "first");
        write(&path, "invalid").unwrap();
        assert!(pool.command("reload", true).contains("unchanged"));
        assert_eq!(pool.snapshot().selected, "second");
        write(&path, "default = 'new'\n[[models]]\nid = 'new'\nmodel = 'new-model'\nprovider = 'subscription'\nvision = true\ntools = true").unwrap();
        assert!(pool.command("reload", true).contains("reloaded"));
        assert_eq!(pool.snapshot().selected, "new");
        assert_eq!(old.config.models.len(), 2);
    }

    #[test]
    fn configuration_rejects_duplicate_ids_and_unbounded_retries() {
        assert!(PoolConfig::parse(include_str!("../../config/models.toml.example")).is_ok());
        let text = "default='a'\n[[models]]\nid='a'\nmodel='test'\nprovider='api_key'\nvision=false\ntools=true\n";
        assert!(PoolConfig::parse(text).is_ok());
        assert!(PoolConfig::parse(&format!("max_attempts=0\n{text}")).is_err());
        assert!(PoolConfig::parse(&format!("{text}[[models]]\nid='a'\nmodel='other'\nprovider='api_key'\nvision=true\ntools=true\n")).is_err());
    }

    #[test]
    fn images_in_earlier_context_still_require_vision() {
        let messages = [
            Message::User {
                content: OneOrMany::one(UserContent::image_base64("aW1n", None, None)),
            },
            Message::user("continue"),
        ];
        assert!(needs_vision(messages.iter()));
        assert!(!needs_vision([&messages[1]]));
    }
}
