//! Feedback lifetime is separate from a Session, provider call and cancellation
//! scope. A manual compact and its resumed task each own their own presentation.

use std::sync::Arc;

use octo_connector_telegram::{TurnFeedback, TURN_END};
use octo_core::{ChannelId, CogitatorContext, ConnectorId, Envelope, EventKind};
use serde_json::json;
use tracing::warn;

use super::AlbertCogitator;
use crate::sessions::{telegram::output_scope, SessionContext};

pub(super) fn fresh_feedback(incoming: &Arc<Envelope>, id: &str) -> Arc<Envelope> {
    Arc::new((**incoming).clone().with_context(TurnFeedback::new(id)))
}

pub(super) fn close_feedback(incoming: &Envelope) {
    if let Some(feedback) = incoming.context.get::<TurnFeedback>() {
        feedback.close();
    }
}

impl AlbertCogitator {
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
