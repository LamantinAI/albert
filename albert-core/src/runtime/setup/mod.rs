use tokio::sync::MutexGuard;

use crate::{
    config::AuthMode, config::Config, db::repo::memory_setup, memory::config::MemoryConfig,
    memory::Memory, models::PoolConfig,
};

use super::AgentRuntime;

fn model_configured(config: &Config) -> bool {
    config
        .models
        .clone()
        .unwrap_or_else(|| PoolConfig::legacy(config))
        .models
        .iter()
        .any(|model| {
            !model.model.trim().is_empty()
                && match model.provider {
                    AuthMode::ApiKey => config.model_key(model.api_key_env.as_deref()).is_some(),
                    AuthMode::Subscription => config.subscription_auth_json.is_file(),
                }
        })
}
impl AgentRuntime {
    /// Serialize a saved memory configuration and its setup checkpoint against
    /// activation's configuration snapshot. Release this lease before activate.
    pub async fn memory_configuration_lease(&self) -> MutexGuard<'_, ()> {
        self.activation.lock().await
    }
    /// Must complete before mutation endpoints/listeners are exposed. Never
    /// classify onboarding from a configuration saved by the first model step.
    pub async fn initialize_memory_setup(&self, browser_first: bool) -> Result<(), String> {
        let config = Config::load_from(&self.path).ok();
        let provisioned = !browser_first
            || config.as_ref().is_some_and(|config| {
                model_configured(config) || matches!(config.memory, MemoryConfig::Mcp { .. })
            });
        let setup = memory_setup::initialize(self.sessions.database().pool(), provisioned)
            .await
            .map_err(|_| "Memory setup could not be initialized.".to_owned())?;
        let mut status = self.status.write().unwrap();
        status.memory_configured = setup.selected();
        status.model_configured = config.as_ref().is_some_and(model_configured);
        Ok(())
    }
    pub(super) async fn refresh_setup_status(&self, config: &Config) -> Result<bool, String> {
        let setup = memory_setup::read(self.sessions.database().pool())
            .await
            .map_err(|_| "Memory setup is unavailable.".to_owned())?;
        let mut status = self.status.write().unwrap();
        status.model_configured = model_configured(config);
        status.memory_configured = setup.selected();
        Ok(setup.selected())
    }
    /// A configuration fingerprint is private; only the match result crosses
    /// the runtime port. It includes the credentials actually used at init.
    pub fn memory_matches(&self, config: &Config) -> bool {
        self.memory_fingerprint
            .read()
            .unwrap()
            .as_ref()
            .is_some_and(|active| active == &Memory::settings_fingerprint(config))
    }
    pub(super) fn memory_phase(&self, phase: &str, error: Option<&str>) {
        let mut status = self.status.write().unwrap();
        status.phase = phase.into();
        status.error = error.map(str::to_owned);
        status.model_ready = self.current().is_some();
    }
}

#[cfg(test)]
mod tests {
    use octo_core::{CogitatorContext, InProcessBus};
    use octo_history::SqliteHistory;
    use std::{fs::create_dir_all, fs::write, sync::atomic::AtomicBool, sync::Arc, time::Duration};
    use tempfile::tempdir;
    use tokio::{spawn, time::timeout};

    use super::AgentRuntime;
    use crate::{
        config::Config, db::repo::memory_setup, service::session::SessionService,
        sessions::settings::SessionSettings, sessions::transport::LocalTransport,
    };

    #[tokio::test]
    async fn saving_first_model_never_initializes_default_memory_before_durable_choice() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("albert.toml");
        write(&path, "").unwrap();
        let vault = dir.path().join("vault");
        let history = Arc::new(
            SqliteHistory::open_retained(dir.path().join("history.db"))
                .await
                .unwrap(),
        );
        let sessions = Arc::new(
            SessionService::from_history(history, SessionSettings::default())
                .await
                .unwrap(),
        );
        let config = Config::load_from(&path).unwrap();
        let authority = config.authorization.build();
        let runtime = AgentRuntime::new(
            path.clone(),
            config,
            sessions,
            authority,
            Arc::new(AtomicBool::new(false)),
            Arc::new(LocalTransport),
        );
        runtime.initialize_memory_setup(true).await.unwrap();
        assert!(!runtime.status().memory_configured);
        write(
            dir.path().join(".env"),
            format!(
                "FIXTURE_KEY=local-only-key\nKAERU_VAULT_PATH={}\n",
                vault.display()
            ),
        )
        .unwrap();
        write(&path, "model='fixture'\nopenai_key_env='FIXTURE_KEY'\n").unwrap();
        runtime.activate().await.unwrap();
        let status = runtime.status();
        assert!(status.model_configured);
        assert!(!status.memory_configured);
        assert!(!status.model_ready);
        assert_eq!(status.phase, "memory_setup");
        assert!(!vault.exists());
        assert!(runtime.current().is_none());
        runtime.initialize_memory_setup(true).await.unwrap();
        assert!(
            !runtime.status().memory_configured,
            "startup retry must not reinterpret the newly saved model as preprovisioning"
        );
        let pending = memory_setup::read(runtime.sessions.database().pool())
            .await
            .unwrap();
        *runtime.context.write().unwrap() = Some(CogitatorContext::new(
            Default::default(),
            Arc::new(InProcessBus::new(64)),
            vec![],
        ));
        create_dir_all(dir.path().join("skills")).unwrap();
        let lease = runtime.memory_configuration_lease().await;
        let worker = runtime.clone();
        let mut activation = spawn(async move { worker.activate().await });
        assert!(timeout(Duration::from_millis(50), &mut activation)
            .await
            .is_err());
        write(&path, "model='fixture'\nopenai_key_env='FIXTURE_KEY'\n[memory]\nbackend='mcp'\ntransport='http'\nurl='http://localhost:1/mcp'\ntimeout_secs=1\n").unwrap();
        memory_setup::accept(runtime.sessions.database().pool(), pending.revision)
            .await
            .unwrap();
        drop(lease);
        assert!(timeout(Duration::from_secs(3), activation)
            .await
            .unwrap()
            .unwrap()
            .is_err());
        assert!(!vault.exists(), "concurrent model activation must see the accepted MCP selection, without embedded fallback");
        assert!(runtime.current().is_none());
        write(&path, "model='fixture'\nopenai_key_env='FIXTURE_KEY'\n").unwrap();
        runtime.activate().await.unwrap();
        assert!(runtime.status().model_ready && runtime.status().memory_configured);
        assert!(vault.exists());
        let active = runtime.current().unwrap();
        let original = Config::load_from(&path).unwrap();
        assert!(runtime.memory_matches(&original));
        write(&path, "model='fixture'\nopenai_key_env='FIXTURE_KEY'\n[memory]\nbackend='mcp'\ntransport='http'\nurl='http://localhost:1/mcp'\n").unwrap();
        runtime.activate().await.unwrap();
        assert!(
            Arc::ptr_eq(&active, &runtime.current().unwrap()),
            "model apply must not swap active memory"
        );
        assert!(!runtime.memory_matches(&Config::load_from(&path).unwrap()));
        write(&path, "model='fixture'\nopenai_key_env='FIXTURE_KEY'\n").unwrap();
        assert!(
            runtime.memory_matches(&Config::load_from(&path).unwrap()),
            "reverting to active settings needs no restart"
        );
    }
}
