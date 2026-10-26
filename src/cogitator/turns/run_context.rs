use std::sync::Arc;

use octo_core::Envelope;
use rig::completion::Message;
use tokio::sync::Mutex;

use super::ChannelState;
use crate::{
    cogitator::{
        channel_of,
        compaction::{CompactOptions, CompactOutcome},
        session_runtime::FrozenInput,
        AlbertCogitator,
    },
    status::StatusFeed,
};

impl AlbertCogitator {
    /// Automatic compaction is an interruptible part of the task. It may only
    /// compact the prefix that preceded this accepted request, even if another
    /// input has since been accepted. The current prompt remains separate.
    pub(super) async fn compact_frozen_input(
        &self,
        incoming: &Envelope,
        history: &mut Vec<Message>,
        prompt: &Message,
        scope: &str,
        state: &Arc<Mutex<ChannelState>>,
        progress: &StatusFeed,
    ) -> Result<(), String> {
        let Some(boundary) = incoming.context.get::<FrozenInput>() else {
            return Ok(());
        };
        let channel = channel_of(incoming);
        let outcome = self
            .compact_before(
                &channel,
                false,
                false,
                progress,
                &CompactOptions::default(),
                Some(boundary.before_id),
            )
            .await?;
        if matches!(outcome, CompactOutcome::Saved) {
            let (mut messages, revision, _) =
                self.snapshot_before(&channel, boundary.before_id).await?;
            messages.extend(boundary.checkpoint.clone());
            let mut locked = state.lock().await;
            let Some(active) = locked
                .active
                .as_mut()
                .filter(|active| active.scope == scope)
            else {
                return Err("Conversation work was superseded.".into());
            };
            *history = messages;
            active.compact_id = revision;
            active.messages = history.clone();
            active.messages.push(prompt.clone());
        }
        Ok(())
    }
}
