use std::{panic::AssertUnwindSafe, time::Duration};

use futures::FutureExt;
use octo_core::CogitatorContext;
use octo_history::SessionId;
use serde_json::to_string;
use tokio::time::timeout;
use tracing::warn;

use crate::{artifacts::Artifacts, cogitator::AlbertCogitator, history::sessions::Backend};

impl AlbertCogitator {
    /// Strict RAM cleanup for a durable erasure. The HTTP host awaits this before
    /// acknowledging its durable cleanup inventory; a failed retry stays pending.
    pub async fn panel_discard_session(
        &self,
        id: &SessionId,
        ctx: &CogitatorContext,
    ) -> Result<(), String> {
        let history = self
            .session_history
            .as_ref()
            .ok_or("Session history is unavailable.")?;
        if *history.handle(id).await.read().await != Backend::Closed {
            return Err("Session cleanup requires a committed backend fence.".into());
        }
        self.cancel_session(id, ctx).await;
        let conversation = ("session".to_owned(), format!("session/{id}"));
        let state = self
            .turns
            .lock()
            .map_err(|_| "Session state cleanup failed.")?
            .get(&conversation)
            .cloned();
        if let Some(state) = state {
            state
                .lock()
                .await
                .discard_content()
                .map_err(|_| "Session status cleanup failed.")?;
        }
        self.children
            .discard_conversation(&conversation)
            .map_err(str::to_owned)?;
        self.scratchpad
            .purge_conversation(&to_string(&conversation).expect("conversation strings serialize"))
            .map_err(str::to_owned)?;
        self.turns
            .lock()
            .map_err(|_| "Session state cleanup failed.")?
            .remove(&conversation);
        if let Some(service) = &self.session_service {
            service.credentials().forget_session(id.as_str());
            service.confirmations().forget_session(id.as_str());
        }
        Ok(())
    }
    pub(in crate::cogitator) async fn on_session_ended(
        &self,
        id: &SessionId,
        ctx: &CogitatorContext,
    ) {
        let Some(history) = &self.session_history else {
            return;
        };
        if *history.handle(id).await.read().await != Backend::Closed {
            warn!("session close token arrived before the backend fence");
            return;
        }
        if !matches!(
            timeout(
                Duration::from_secs(2),
                AssertUnwindSafe(self.panel_discard_session(id, ctx)).catch_unwind()
            )
            .await,
            Ok(Ok(Ok(())))
        ) {
            warn!("session cancellation reached its cleanup deadline or was partial");
        }
        let conversation = ("session".to_owned(), format!("session/{id}"));
        if Artifacts::discard_conversation(&self.config.code_workspace, &conversation).is_err() {
            warn!("session artifact cleanup failed");
        }
    }
}
