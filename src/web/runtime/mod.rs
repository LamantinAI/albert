//! The control plane stays available while cognition is unconfigured or fails.
mod health;
mod proxy;
pub(crate) use proxy::RuntimeCogitator;

use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, RwLock,
    },
};

use octo_approval::Authority;
use octo_core::CogitatorContext;
use octo_openai_auth::SubscriptionAuth;
use rand::random;
use tokio::sync::Mutex;

use crate::{
    api::models::RuntimeStatus,
    cogitator::AlbertCogitator,
    config::{AuthMode, Config},
    memory::Memory,
    models::PoolConfig,
    prompt::PromptFiles,
    scratchpad::ScratchpadStore,
    service::session::SessionService,
    skills::SkillStore,
};

pub(crate) struct AgentRuntime {
    pub path: PathBuf,
    pub base: Config,
    pub sessions: Arc<SessionService>,
    pub authority: Authority,
    pub subscription: Arc<SubscriptionAuth>,
    active: RwLock<Option<Arc<AlbertCogitator>>>,
    context: RwLock<Option<CogitatorContext>>,
    status: RwLock<RuntimeStatus>,
    activation: Mutex<()>,
    pub restart_requested: Arc<AtomicBool>,
}

impl AgentRuntime {
    pub fn new(
        path: PathBuf,
        base: Config,
        sessions: Arc<SessionService>,
        authority: Authority,
        restart_requested: Arc<AtomicBool>,
    ) -> Arc<Self> {
        Arc::new(Self {
            path,
            subscription: Arc::new(
                SubscriptionAuth::new(base.subscription_auth_json.clone()).with_login_hint(
                    "Connect subscription credentials in settings or run albert login",
                ),
            ),
            base,
            sessions,
            authority,
            active: RwLock::new(None),
            context: RwLock::new(None),
            status: RwLock::new(RuntimeStatus {
                phase: "starting".into(),
                model_ready: false,
                runtime_epoch: format!("{:032x}", random::<u128>()),
                error: None,
            }),
            activation: Mutex::new(()),
            restart_requested,
        })
    }

    pub fn status(&self) -> RuntimeStatus {
        self.status.read().unwrap().clone()
    }
    pub fn current(&self) -> Option<Arc<AlbertCogitator>> {
        self.active.read().unwrap().clone()
    }
    pub fn context(&self) -> Option<CogitatorContext> {
        self.context.read().unwrap().clone()
    }

    fn unavailable(&self, message: Option<&str>) {
        let mut state = self.status.write().unwrap();
        state.model_ready = self.current().is_some();
        state.phase = if message.is_some() {
            "runtime_error"
        } else {
            "setup"
        }
        .into();
        state.error = message.map(str::to_owned);
    }

    pub async fn activate(&self) -> Result<(), String> {
        let _activation = self.activation.lock().await;
        if !self.path.exists() {
            self.unavailable(None);
            return Ok(());
        }
        let config = Config::load_from(&self.path).map_err(|_| {
            self.unavailable(Some("Agent configuration is invalid. Review Settings."));
            "Agent configuration is invalid. Review Settings.".to_owned()
        })?;
        let pool = config
            .models
            .clone()
            .unwrap_or_else(|| PoolConfig::legacy(&config));
        let usable = pool.models.iter().any(|model| match model.provider {
            AuthMode::ApiKey => {
                !model.model.trim().is_empty()
                    && config.model_key(model.api_key_env.as_deref()).is_some()
            }
            AuthMode::Subscription => {
                !model.model.trim().is_empty() && config.subscription_auth_json.is_file()
            }
        });
        if !usable {
            self.unavailable(None);
            return Ok(());
        }
        let Some(context) = self.context() else {
            return Err("Runtime is still starting. Retry Apply shortly.".into());
        };
        if let Some(current) = self.current() {
            current.panel_reload(&config, &context);
        } else {
            let vault = Some(config.embedded_vault_path());
            let memory = Memory::open_at(&config, vault).await.map_err(|_| {
                self.unavailable(Some(
                    "Agent memory could not be initialized. Review memory settings.",
                ));
                "Agent memory could not be initialized. Review memory settings.".to_owned()
            })?;
            let capabilities = if config.subscription_auth_json.exists() {
                vec!["subscription", "imagegen"]
            } else {
                Vec::new()
            };
            let skills = SkillStore::load(
                config.skills_dir.clone(),
                config.skills_cache,
                config.skills_page,
                &capabilities,
            );
            let scratchpad = ScratchpadStore::with_limits(config.scratchpad.clone())?;
            let prompt = PromptFiles::new(config.soul_path.clone(), config.system_path.clone());
            let agent = AlbertCogitator::new(
                "albert",
                config,
                self.sessions.journal_router().clone(),
                memory,
                scratchpad,
                skills,
                prompt,
                self.subscription.clone(),
                self.authority.clone(),
            )
            .with_sessions(self.sessions.clone());
            agent.panel_initialize(&context);
            *self.active.write().unwrap() = Some(agent);
        }
        let mut state = self.status.write().unwrap();
        state.phase = "ready".into();
        state.model_ready = true;
        state.error = None;
        Ok(())
    }

    pub fn request_restart(&self) -> Result<(), String> {
        let context = self.context().ok_or("Runtime is not started.")?;
        self.restart_requested.store(true, Ordering::Release);
        context.shutdown.cancel();
        Ok(())
    }
}
