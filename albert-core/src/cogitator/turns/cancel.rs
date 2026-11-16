use octo_core::CogitatorContext;
use octo_history::SessionId;
use octo_rig::carry_out_cancel;
use tracing::warn;

use crate::{cogitator::AlbertCogitator, history::tool_trace, messaging::store::WorkTerminal};

impl AlbertCogitator {
    pub async fn panel_active(&self, session: &SessionId) -> Option<String> {
        let key = ("session".to_owned(), format!("session/{session}"));
        let state = self.turns.lock().unwrap().get(&key).cloned()?;
        let state = state.lock().await;
        state
            .active
            .as_ref()
            .map(|active| active.feedback.id.to_string())
    }

    pub async fn panel_cancel_exact(
        &self,
        session: &SessionId,
        attempt: &str,
        ctx: &CogitatorContext,
    ) -> bool {
        let key = ("session".to_owned(), format!("session/{session}"));
        let state = self.turns.lock().unwrap().get(&key).cloned();
        let Some(state) = state else {
            return false;
        };
        let mut locked = state.lock().await;
        if !locked
            .active
            .as_ref()
            .is_some_and(|active| active.feedback.id.to_string() == attempt)
        {
            return false;
        }
        if let Some(service) = &self.session_service {
            service.credentials().cancel_session(session.as_str());
            service.confirmations().cancel_session(session.as_str());
        }
        let Some(previous) = locked.interrupt().await else {
            return false;
        };
        locked.last_outcome = Some("cancelled".into());
        self.end_feedback(&previous.feedback, ctx).await;
        self.cancel_scope(&previous.scope, ctx).await;
        self.children.cancel_channel(&key).await;
        if let Some(record) = tool_trace(&previous.checkpoint) {
            if self.history.append(&key.1, &[record]).await.is_err() {
                warn!("could not persist stopped tool checkpoint");
            }
        }
        true
    }

    pub(in crate::cogitator) async fn cancel_channel(
        &self,
        key: &(String, String),
        ctx: &CogitatorContext,
    ) -> bool {
        if let Some(service) = &self.session_service {
            if let Some(session) = key.1.strip_prefix("session/") {
                service.credentials().cancel_session(session);
                service.confirmations().cancel_session(session);
            }
        }
        let state = self.turns.lock().unwrap().get(key).cloned();
        let Some(state) = state else {
            return self.children.cancel_channel(key).await;
        };
        let mut locked = state.lock().await;
        let message_cancelled = if let Some(message) = locked.message.take() {
            message.stop(self, ctx, WorkTerminal::Cancelled).await;
            true
        } else {
            false
        };
        let previous = locked.interrupt().await;
        if let Some(previous) = &previous {
            self.end_feedback(&previous.feedback, ctx).await;
            self.cancel_scope(&previous.scope, ctx).await;
        }
        let children_cancelled = self.children.cancel_channel(key).await;
        if previous.is_some() || children_cancelled || message_cancelled {
            locked.last_outcome = Some("cancelled".into());
        }
        let Some(previous) = previous else {
            return children_cancelled || message_cancelled;
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

    pub(in crate::cogitator) async fn cancel_scope(&self, scope: &str, ctx: &CogitatorContext) {
        if let Some(service) = &self.session_service {
            service.credentials().cancel_origin(scope);
            service.confirmations().cancel_origin(scope);
        }
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

impl AlbertCogitator {
    /// Current execution, independently of the chat selected for presentation.
    pub async fn panel_activity(
        &self,
        session: &SessionId,
        owner: bool,
    ) -> (bool, Option<String>, usize) {
        let key = ("session".to_owned(), format!("session/{session}"));
        let state = self.turns.lock().unwrap().get(&key).cloned();
        let children = self
            .children
            .visible(&key, owner)
            .iter()
            .filter(|run| run.status().as_str() == Some("running"))
            .count();
        let Some(state) = state else {
            return (children > 0, None, children);
        };
        let locked = state.lock().await;
        let running = locked.active.is_some()
            || locked
                .message
                .as_ref()
                .is_some_and(|task| !task.is_finished())
            || children > 0;
        (
            running,
            if running {
                None
            } else {
                locked
                    .message
                    .as_ref()
                    .and_then(|task| task.outcome())
                    .map(str::to_owned)
                    .or_else(|| locked.last_outcome.clone())
            },
            children,
        )
    }
}
