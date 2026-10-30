use std::{
    collections::HashMap,
    sync::{atomic::AtomicU64, Arc, Mutex},
};

use octo_approval::Authority;
use octo_core::ConnectorId;
use octo_openai_auth::SubscriptionAuth;

use super::{messaging::MessagingRuntime, AlbertCogitator};
use crate::{
    config::Config, history::HistoryStore, memory::Memory, messaging::store::MessageStore,
    models::ModelPool, prompt::PromptFiles, scratchpad::ScratchpadStore,
    service::session::SessionService, skills::SkillStore,
};

impl AlbertCogitator {
    pub fn new(
        id: impl Into<String>,
        config: Config,
        history: Arc<dyn HistoryStore>,
        memory: Memory,
        scratchpad: Arc<ScratchpadStore>,
        skills: Arc<SkillStore>,
        prompt: Arc<PromptFiles>,
        auth: Arc<SubscriptionAuth>,
        authority: Authority,
    ) -> Arc<Self> {
        let id = id.into();
        let models = ModelPool::new(&config);
        Arc::new(Self {
            authority,
            children: Default::default(),
            models,
            self_source: ConnectorId::new(format!("cogitator/{id}")),
            id,
            config,
            history,
            session_service: None,
            session_history: None,
            request_gates: Default::default(),
            messaging: None,
            memory,
            scratchpad,
            skills,
            prompt,
            auth,
            turns: Mutex::new(HashMap::new()),
            turn_seq: AtomicU64::new(0),
        })
    }

    pub(crate) fn with_sessions(mut self: Arc<Self>, service: Arc<SessionService>) -> Arc<Self> {
        let history = service.journal_router().clone();
        let host = Arc::get_mut(&mut self).expect("install sessions before sharing the cogitator");
        host.authority = host
            .authority
            .clone()
            .with_scope_validator(service.scope_registry());
        host.history = history.clone();
        if host.config.sessions.messaging.enabled {
            let settings = host.config.sessions.messaging.clone();
            let store = MessageStore::new(
                service.history().clone(),
                history.lifecycle_gate(),
                settings.limits.clone(),
            )
            .expect("validated messaging settings");
            host.messaging = Some(Arc::new(MessagingRuntime::new(
                store,
                settings,
                service.delivery_adapters(),
            )));
        }
        host.session_service = Some(service);
        host.session_history = Some(history);
        self
    }
}
