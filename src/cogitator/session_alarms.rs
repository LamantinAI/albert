use std::sync::Arc;

use octo_core::{ChannelId, CogitatorContext, ConnectorId, Envelope, EventKind};
use octo_history::{SessionId, SessionOrigin, SessionStore};
use serde_json::Value;
use tracing::warn;

use super::{AlbertCogitator, UserInput, SCHEDULER_ID};
use crate::sessions::{error::SessionError, ReplyRoute};

impl AlbertCogitator {
    pub(super) async fn on_session_alarm(
        self: &Arc<Self>,
        incoming: Arc<Envelope>,
        ctx: &CogitatorContext,
    ) {
        if incoming.source.as_str() != SCHEDULER_ID {
            warn!("untrusted alarm source rejected");
            return;
        }
        let result = async {
            let service = self.session_service.as_ref().ok_or(SessionError::Denied)?;
            let payload = incoming.payload_as::<Value>().ok_or(SessionError::InvalidInput("alarm payload"))?;
            let connector = payload.get("reply_via").and_then(Value::as_str).ok_or(SessionError::InvalidInput("alarm reply connector"))?;
            let channel = payload.get("channel").and_then(Value::as_str).ok_or(SessionError::InvalidInput("alarm reply channel"))?;
            let id = match payload.get("albert_session_id").and_then(Value::as_str) {
                Some(value) => {
                    if payload.get("albert_session_host").and_then(Value::as_str) != Some("host") { return Err(SessionError::Denied); }
                    SessionId::parse(value)?
                }
                None => {
                    // Old reminders refer to the immutable verified channel
                    // origin. A mutable private selector is never consulted.
                    service.history().get_default_session(&SessionOrigin { connector: connector.into(), channel: channel.into() }).await?.ok_or(SessionError::NotFound)?.id
                }
            };
            let route = ReplyRoute::new(connector, channel, Some(incoming.id.to_string()))?;
            let session = service.context_for_reminder(&id, route).await?;
            let task = payload.get("task").and_then(Value::as_str).ok_or(SessionError::InvalidInput("alarm task"))?;
            let text = format!("Reminder due for {task:?}. Write a short reminder. Do not schedule another reminder.");
            let mut envelope = (*incoming).clone();
            envelope.source = ConnectorId::new(connector);
            envelope.channel = Some(ChannelId::new(channel));
            envelope.kind = EventKind::from_static("chat.message");
            envelope.channel_metadata = None;
            envelope.tags.clear();
            envelope.context.insert(session);
            self.clone().spawn_turn(Arc::new(envelope), UserInput { text, images: Vec::new(), seed: None, voice: None }, ctx).await;
            Ok::<(), SessionError>(())
        }.await;
        if let Err(error) = result {
            warn!(%error, "reminder session admission failed");
        }
    }
}
