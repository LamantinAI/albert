//! Shared presentation lifetime. Execution, journals and diagnostics never move.
use std::sync::{Arc, Mutex};

use octo_connector_telegram::TurnFeedback;
use octo_core::{ChannelId, ConnectorId, Envelope};
use octo_history::SessionKind;
use rand::random;

use super::StatusFeed;
use crate::sessions::{telegram::output_scope, SessionContext};

#[derive(Clone, Default)]
pub(crate) struct PresentationBinding {
    pub feedback: Option<TurnFeedback>,
    pub context: Option<SessionContext>,
    pub attempt: Option<String>,
    route: Option<(ConnectorId, Option<ChannelId>)>,
}
impl PresentationBinding {
    pub(crate) fn output(&self, mut output: Envelope) -> Envelope {
        if let Some((target, channel)) = &self.route {
            output.target = Some(target.clone());
            output.channel = channel.clone();
        }
        if let Some(context) = &self.context {
            output = output
                .with_target(ConnectorId::new(context.reply_route().connector()))
                .with_channel(ChannelId::new(context.reply_route().channel()))
                .with_tag("session_id", context.session_id().as_str())
                .with_context(context.clone())
                .with_context(output_scope(context, self.attempt.clone()));
        }
        if let Some(feedback) = &self.feedback {
            output = output.with_context(feedback.clone());
        }
        output
    }
    fn close(&self) {
        if let Some(feedback) = &self.feedback {
            feedback.close();
        }
    }
}
pub(super) enum PresentationState {
    Attached(Box<PresentationBinding>),
    Detached,
    Finished,
}
impl Default for PresentationState {
    fn default() -> Self {
        Self::Attached(Box::default())
    }
}
impl StatusFeed {
    pub(crate) fn with_presentation_route(
        mut self,
        target: ConnectorId,
        channel: Option<ChannelId>,
    ) -> Self {
        self.edit_binding(|binding| binding.route = Some((target, channel)));
        self
    }
    pub(super) fn edit_binding(&mut self, edit: impl FnOnce(&mut PresentationBinding)) {
        if let PresentationState::Attached(binding) = &mut *self.presentation.lock().unwrap() {
            edit(binding);
        }
    }
    pub(crate) fn presentation_binding(&self) -> Option<PresentationBinding> {
        match &*self.presentation.lock().unwrap() {
            PresentationState::Attached(binding) => Some((**binding).clone()),
            _ => None,
        }
    }
    pub(crate) fn detach_presentation(
        &self,
        matches: impl FnOnce(&SessionContext) -> bool,
    ) -> Option<PresentationBinding> {
        let mut state = self.presentation.lock().unwrap();
        let PresentationState::Attached(binding) = &*state else {
            return None;
        };
        if !binding.context.as_ref().is_some_and(matches) {
            return None;
        }
        let previous = (**binding).clone();
        previous.close();
        *state = PresentationState::Detached;
        Some(previous)
    }
    pub(crate) fn finish_presentation(&self) -> Option<PresentationBinding> {
        let mut state = self.presentation.lock().unwrap();
        let previous = match &*state {
            PresentationState::Attached(binding) => Some((**binding).clone()),
            _ => None,
        };
        if let Some(binding) = &previous {
            binding.close();
        }
        *state = PresentationState::Finished;
        previous
    }
    pub(crate) fn with_reattach(mut self, allowed: bool) -> Self {
        self.reattachable = allowed;
        self
    }
    pub(crate) fn initially_detached(self) -> Self {
        *self.presentation.lock().unwrap() = PresentationState::Detached;
        self
    }
    pub(crate) fn share_presentation(mut self, source: &Self) -> Self {
        self.presentation = source.presentation.clone();
        self
    }
    /// Host-only: a private saved Session may reconnect only to its original route.
    pub(crate) fn can_attach_presentation(&self, context: &SessionContext) -> bool {
        let Some(original) = &self.session_context else {
            return false;
        };
        if !self.reattachable
            || self.feed.is_none()
            || !self.is_live()
            || !context.is_live()
            || !context.is_private()
            || context.session().kind == SessionKind::Temporary
            || original.session_id() != context.session_id()
            || original.private_user_id().ok() != context.private_user_id().ok()
            || original.reply_route().connector() != context.reply_route().connector()
            || original.reply_route().channel() != context.reply_route().channel()
        {
            return false;
        }
        true
    }
    pub(crate) fn attach_presentation(&self, context: &SessionContext) -> bool {
        if !self.can_attach_presentation(context) {
            return false;
        }
        let mut state = self.presentation.lock().unwrap();
        match &*state {
            PresentationState::Finished => return false,
            PresentationState::Attached(binding)
                if binding.feedback.as_ref().is_some_and(TurnFeedback::is_open) =>
            {
                return false
            }
            _ => {}
        }
        let attempt = format!("presentation/{:032x}", random::<u128>());
        *state = PresentationState::Attached(Box::new(PresentationBinding {
            feedback: Some(TurnFeedback::new(&attempt)),
            context: Some(context.clone()),
            attempt: Some(attempt),
            route: None,
        }));
        true
    }
}

pub(super) fn shared_state() -> Arc<Mutex<PresentationState>> {
    Arc::new(Mutex::new(PresentationState::default()))
}
