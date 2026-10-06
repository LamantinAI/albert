use std::sync::Arc;

use kaeru_core::Store;
use kaeru_rig::KaeruMemory;
use octo_core::{CogitatorContext, InProcessBus};
use octo_openai_auth::SubscriptionAuth;
use tempfile::tempdir;

use super::AlbertCogitator;
use crate::{
    config::{AuthMode, Config},
    history::InMemoryHistory,
    memory::Memory,
    prompt::PromptFiles,
    scratchpad::ScratchpadStore,
    skills::SkillStore,
};

pub(super) fn fixture() -> (Arc<AlbertCogitator>, CogitatorContext, Arc<InMemoryHistory>) {
    let dir = tempdir().unwrap();
    let path = dir.path().to_path_buf();
    let config = Config {
        model_pool: None,
        models: None,
        memory: Default::default(),
        model: "unused".into(),
        auth: AuthMode::ApiKey,
        base_url: String::new(),
        api_key: None,
        subscription_auth_json: path.join("auth.json"),
        subscription_base_url: String::new(),
        multimodal: true,
        stream_status: false,
        history: None,
        scheduler_state_path: path.join("scheduler"),
        reflection_secs: 0,
        soul_path: path.join("soul.md"),
        system_path: path.join("system.md"),
        max_tool_turns: 5,
        connectors_manifest: path.join("octo.toml"),
        skills_dir: path.join("skills"),
        skills_cache: 5,
        skills_page: 10,
        timezone: "UTC".parse().unwrap(),
        clouds: Default::default(),
        clouds_default: None,
        code_workspace: path.clone(),
        deploy_dir: path.clone(),
    };
    let history = Arc::new(InMemoryHistory::new(30));
    let memory = Memory::Embedded {
        memory: KaeruMemory::with_initiative(Arc::new(Store::open_in_memory().unwrap()), "albert"),
        clouds: false,
    };
    let agent = AlbertCogitator::new(
        "test",
        config,
        history.clone(),
        memory,
        ScratchpadStore::new(),
        SkillStore::load(path.join("skills"), 5, 10, &[]),
        PromptFiles::new(path.join("soul"), path.join("system")),
        Arc::new(SubscriptionAuth::new(path.join("auth"))),
    );
    // No scheduler: attempts block in the preflight request, never call an LLM.
    let ctx = CogitatorContext::new(Default::default(), Arc::new(InProcessBus::new(64)), vec![]);
    (agent, ctx, history)
}
