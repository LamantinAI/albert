use std::{panic::AssertUnwindSafe, time::Duration};

use futures::FutureExt;

use octo_core::CogitatorContext;
use octo_history::SessionId;
use serde_json::to_string;
use tokio::time::timeout;
use tracing::warn;

use crate::{artifacts::Artifacts, cogitator::AlbertCogitator, history::sessions::Backend};

impl AlbertCogitator {
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
        let conversation = ("session".to_owned(), format!("session/{id}"));
        let state = self
            .turns
            .lock()
            .unwrap_or_else(|error| {
                warn!("session gate state was poisoned during cleanup");
                error.into_inner()
            })
            .get(&conversation)
            .cloned();
        if let Some(state) = state {
            if state.lock().await.discard_content().is_err() {
                warn!("session status cleanup was partial");
            }
        }
        if !matches!(
            timeout(
                Duration::from_secs(2),
                AssertUnwindSafe(self.cancel_session(id, ctx)).catch_unwind()
            )
            .await,
            Ok(Ok(_))
        ) {
            warn!("session cancellation reached its cleanup deadline");
        }
        if self.children.discard_conversation(&conversation).is_err() {
            warn!("session child cleanup was partial");
        }
        if self
            .scratchpad
            .purge_conversation(&to_string(&conversation).expect("conversation strings serialize"))
            .is_err()
        {
            warn!("session scratchpad cleanup was partial");
        }
        if Artifacts::discard_conversation(&self.config.code_workspace, &conversation).is_err() {
            warn!("session artifact cleanup failed");
        }
        self.turns
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&conversation);
    }
}
