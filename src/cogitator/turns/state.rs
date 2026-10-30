use super::{ChannelState, Interrupted};
use crate::{
    cogitator::{feedback::close_feedback, messaging::MessageTask},
    history::journal_messages,
    status::StatusFeed,
};
#[cfg(test)]
use octo_core::Envelope;
#[cfg(test)]
use std::sync::Arc;
impl ChannelState {
    #[cfg(test)]
    pub(in crate::cogitator) fn feedback(&self) -> Option<&Arc<Envelope>> {
        self.active.as_ref().map(|active| &active.feedback)
    }
    pub(in crate::cogitator) fn presentation_feed(&self) -> Option<StatusFeed> {
        self.active
            .as_ref()
            .filter(|active| !active.task.is_finished())
            .map(|active| active.feed.clone())
            .or_else(|| {
                self.message
                    .as_ref()
                    .filter(|task| !task.is_finished())
                    .map(MessageTask::presentation_feed)
            })
    }
    pub(in crate::cogitator) fn native_active(&self) -> bool {
        self.active.is_some()
    }
    pub(in crate::cogitator) fn is_idle(&mut self) -> bool {
        if self.message.as_ref().is_some_and(MessageTask::is_finished) {
            self.message = None;
        }
        self.active.is_none() && self.message.is_none()
    }
    pub(in crate::cogitator) fn discard_content(&mut self) -> Result<(), &'static str> {
        if let Some(active) = self.active.as_mut() {
            let result = active.feed.discard();
            active.messages.clear();
            active.continuation = None;
            return result;
        }
        Ok(())
    }
    pub(super) async fn interrupt(&mut self) -> Option<Interrupted> {
        let active = self.active.take()?;
        close_feedback(&active.feedback);
        active.task.abort();
        let _ = active.task.await; // no old hooks/replies may race the checkpoint
        let checkpoint = active.feed.checkpoint();
        let mut messages = active.messages;
        messages.extend(journal_messages(&checkpoint));
        Some(Interrupted {
            tool_policy: active.tool_policy,
            feedback: active.feedback,
            continuation: active.continuation,
            compact_id: active.compact_id,
            scope: active.scope,
            messages,
            checkpoint,
            owner: active.owner,
        })
    }
}
