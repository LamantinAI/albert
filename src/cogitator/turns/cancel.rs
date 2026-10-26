use octo_core::CogitatorContext;
use octo_rig::carry_out_cancel;
use tracing::warn;

use crate::{cogitator::AlbertCogitator, history::tool_trace};

impl AlbertCogitator {
    pub(in crate::cogitator) async fn cancel_channel(
        &self,
        key: &(String, String),
        ctx: &CogitatorContext,
    ) -> bool {
        let state = self.turns.lock().unwrap().get(key).cloned();
        let Some(state) = state else {
            return self.children.cancel_channel(key).await;
        };
        let mut locked = state.lock().await;
        let previous = locked.interrupt().await;
        if let Some(previous) = &previous {
            self.end_feedback(&previous.feedback, ctx).await;
            self.cancel_scope(&previous.scope, ctx).await;
        }
        let children_cancelled = self.children.cancel_channel(key).await;
        let Some(previous) = previous else {
            return children_cancelled;
        };
        // Caller records the explicit /cancel after this checkpoint.
        let channel = &key.1;
        if let Some(record) = tool_trace(&previous.checkpoint) {
            if let Err(error) = self.history.append(channel, &[record]).await {
                warn!(%error, "could not persist interrupted tool history");
            }
        }
        true
    }

    pub(super) async fn cancel_scope(&self, scope: &str, ctx: &CogitatorContext) {
        if let Err(error) = carry_out_cancel(&ctx.bus(), &self.self_source, scope).await {
            warn!(%error, %scope, "failed to publish cancellation");
        }
    }

    pub(in crate::cogitator) async fn stop_turns(&self, ctx: &CogitatorContext) {
        let keys: Vec<_> = self.turns.lock().unwrap().keys().cloned().collect();
        for key in keys {
            self.cancel_channel(&key, ctx).await;
        }
        self.children.cancel_all().await;
    }
}
