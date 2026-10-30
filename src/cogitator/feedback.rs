//! Feedback lifetime is separate from a Session, provider call and cancellation
//! scope. A manual compact and its resumed task each own their own presentation.

use std::sync::Arc;

use octo_connector_telegram::{TurnFeedback, TURN_END};
use octo_core::{ChannelId, CogitatorContext, ConnectorId, Envelope, EventKind};
use serde_json::json;
use tracing::warn;

use super::AlbertCogitator;
use crate::{
    sessions::{
        presentation::{PresentationDetached, ScheduledNotification},
        telegram::output_scope,
        SessionContext,
    },
    status::{PresentationBinding, StatusFeed},
};

pub(super) fn close_feedback(incoming: &Envelope) {
    if let Some(feedback) = incoming.context.get::<TurnFeedback>() {
        feedback.close();
    }
}

impl AlbertCogitator {
    pub(super) fn fresh_feedback(&self, incoming: &Arc<Envelope>, id: &str) -> Arc<Envelope> {
        let incoming = Arc::new((**incoming).clone().with_context(TurnFeedback::new(id)));
        self.fence_feedback(&incoming);
        incoming
    }
    pub(super) fn fence_feedback(&self, incoming: &Envelope) {
        if let (Some(service), Some(context)) = (
            &self.session_service,
            incoming.context.get::<SessionContext>(),
        ) {
            if service.presentations().detached(context)
                || (incoming.context.get::<ScheduledNotification>().is_none()
                    && !service.presentations().active(context))
            {
                close_feedback(incoming);
            }
        }
    }
    pub(super) async fn detach_presentation(
        &self,
        detached: &PresentationDetached,
        ctx: &CogitatorContext,
    ) {
        // The service recorded the monotonic cutoff before this scan. Fresh
        // generations are fenced even when their slot did not exist during it.
        let key = (
            "session".into(),
            format!("session/{}", detached.session_id()),
        );
        let state = self.turns.lock().unwrap().get(&key).cloned();
        let binding = if let Some(state) = state {
            let locked = state.lock().await;
            locked
                .presentation_feed()
                .and_then(|feed| feed.detach_presentation(|context| detached.matches(context)))
        } else {
            None
        };
        if let Some(binding) = binding {
            self.end_binding(binding, ctx).await;
        }
    }

    pub(super) async fn open_presentation(&self, context: &SessionContext, ctx: &CogitatorContext) {
        let Some(service) = &self.session_service else {
            return;
        };
        if !service.presentations().current(context) {
            return;
        }
        let key = (
            "session".into(),
            format!("session/{}", context.session_id()),
        );
        let state = self.turns.lock().unwrap().get(&key).cloned();
        let Some(state) = state else { return };
        let (previous, current) = {
            let locked = state.lock().await;
            if !service.presentations().current(context) {
                return;
            }
            let Some(feed) = locked.presentation_feed() else {
                return;
            };
            if !feed.can_attach_presentation(context) {
                return;
            }
            // /open may overtake the old detach event; cutoff invalidates its token.
            let previous =
                feed.detach_presentation(|bound| service.presentations().detached(bound));
            let attached = feed.attach_presentation(context);
            let current = if attached && service.presentations().current(context) {
                feed.presentation_binding()
            } else {
                if attached {
                    feed.detach_presentation(|_| true);
                }
                None
            };
            (previous, current)
        };
        // No Session gate or shared presentation lock is held during publication.
        if let Some(previous) = previous {
            self.end_binding(previous, ctx).await;
        }
        if let Some(binding) = current {
            let typing = binding.output(Envelope::new(
                self.self_source.clone(),
                EventKind::new("chat.typing"),
                json!({}),
            ));
            let status = binding.output(Envelope::new(
                self.self_source.clone(),
                EventKind::new("chat.status"),
                "Work is still running".to_owned(),
            ));
            let _ = ctx.publish(typing).await;
            let _ = ctx.publish(status).await;
        }
    }

    pub(super) async fn finish_feed(&self, feed: &StatusFeed, ctx: &CogitatorContext) {
        if let Some(binding) = feed.finish_presentation() {
            self.end_binding(binding, ctx).await;
        }
    }
    pub(super) async fn end_binding(&self, binding: PresentationBinding, ctx: &CogitatorContext) {
        if binding.feedback.is_none() {
            return;
        }
        let output = binding.output(Envelope::new(
            self.self_source.clone(),
            EventKind::new(TURN_END),
            json!({}),
        ));
        if ctx.publish(output).await.is_err() {
            warn!("could not publish feedback completion");
        }
    }

    /// Always use the captured ingress, never the channel's current selector.
    pub(super) fn feedback_output(&self, incoming: &Envelope, mut output: Envelope) -> Envelope {
        if let Some(session) = incoming.context.get::<SessionContext>() {
            output.target = Some(ConnectorId::new(session.reply_route().connector()));
            output.channel = Some(ChannelId::new(session.reply_route().channel()));
            output = output
                .with_tag("session_id", session.session_id().as_str())
                .with_context(session.clone())
                .with_context(output_scope(session, Some(incoming.id.to_string())));
        } else {
            output.target = Some(incoming.source.clone());
            output.channel = incoming.channel.clone();
        }
        if let Some(feedback) = incoming.context.get::<TurnFeedback>() {
            output = output.with_context(feedback.clone());
        }
        output
    }

    /// Metadata cleanup must also reach the connector after Session fencing.
    pub(super) async fn end_feedback(&self, incoming: &Envelope, ctx: &CogitatorContext) {
        if let Some(feed) = incoming.context.get::<StatusFeed>() {
            self.finish_feed(feed, ctx).await;
            return;
        }
        if incoming.context.get::<TurnFeedback>().is_none() {
            return;
        }
        close_feedback(incoming);
        let output = self.feedback_output(
            incoming,
            Envelope::new(
                self.self_source.clone(),
                EventKind::new(TURN_END),
                json!({}),
            ),
        );
        if ctx.publish(output).await.is_err() {
            warn!("could not publish feedback completion");
        }
    }
}
