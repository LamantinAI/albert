use std::sync::Arc;

use octo_core::{CogitatorContext, ConnectorId, Envelope};
use tokio::spawn;

use super::AlbertCogitator;
use crate::{
    commands::{menu, publish_menu, SET_COMMANDS},
    config::Config,
    models::{PoolConfig, Snapshot},
    routines::seed_base_routine,
};

impl AlbertCogitator {
    pub(crate) fn panel_initialize(&self, ctx: &CogitatorContext) {
        // Seed Albert's base routines (memory reflection) once the scheduler is up.
        spawn(seed_base_routine(
            ctx.bus(),
            self.self_source.clone(),
            self.config.reflection_secs,
        ));
        // Publish the command menu on every channel that takes one.
        let channels: Vec<ConnectorId> = ctx
            .connectors()
            .iter()
            .filter(|c| {
                c.capabilities
                    .event_kinds_accept
                    .iter()
                    .any(|k| k.as_str() == SET_COMMANDS)
            })
            .map(|c| c.id.clone())
            .collect();
        if !channels.is_empty() {
            spawn(publish_menu(
                ctx.bus(),
                self.self_source.clone(),
                channels,
                menu(&self.skills.commands()),
            ));
        }
    }
    pub(crate) async fn panel_handle(
        self: Arc<Self>,
        event: Arc<Envelope>,
        ctx: &CogitatorContext,
    ) {
        self.handle(event, ctx).await;
    }
    pub(crate) async fn panel_stop(&self, ctx: &CogitatorContext) {
        self.stop_turns(ctx).await;
    }
    pub(crate) fn panel_prefer(&self, id: &str) -> Result<(), String> {
        self.models.panel_prefer(id)
    }
    pub(crate) fn panel_models(&self) -> Snapshot {
        self.models.snapshot()
    }
    pub(crate) fn panel_reload(&self, config: &Config, ctx: &CogitatorContext) {
        let pool = config
            .models
            .clone()
            .unwrap_or_else(|| PoolConfig::legacy(config));
        self.models.apply_panel(pool, config.model_pool.clone());
        let capabilities: Vec<&str> = if config.subscription_auth_json.exists() {
            vec!["subscription", "imagegen"]
        } else {
            Vec::new()
        };
        self.skills
            .refresh(config.skills_dir.clone(), &capabilities);
        self.panel_initialize(ctx);
    }
}
