//! Telegram wire mapping stays at the authenticated transport edge.
use std::sync::Arc;

use albert_core::{
    db::entity::ExcursionKey, db::entity::NativePosition,
    sessions::output::output_scope as domain_scope, sessions::transport::FeedbackToken,
    sessions::transport::HostTransport, sessions::transport::NativeConstituent,
    sessions::transport::NativeIdentityError, sessions::transport::NativeIngress,
    sessions::transport::NativeNavigation, sessions::transport::OutputScope,
    sessions::transport::TurnFeedback, sessions::SessionContext,
};
use octo_connector_telegram::{
    IngressScope, NativeBatch, NativeFrame, TelegramOutputScope, TurnFeedback as TelegramFeedback,
};
use octo_core::Envelope;
use octo_history::RequestAlias;
use serde_json::{json, to_vec};
use sha2::{Digest, Sha256};

pub(crate) fn request_alias(frame: &NativeFrame) -> RequestAlias {
    let issuer = to_vec(&(frame.bot_id, frame.connector.as_str(), frame.chat_id))
        .expect("native identity serializes");
    RequestAlias {
        namespace: format!("telegram.native.{:x}", Sha256::digest(issuer)),
        idempotency_key: format!(
            "update:{}:message:{}:at:{}",
            frame.update_id, frame.message_id, frame.date
        ),
        digest: frame.digest.clone(),
    }
}

pub(crate) fn navigation(frame: &NativeFrame) -> NativeNavigation {
    NativeNavigation {
        identity: if frame.bot_id == 0
            || frame.message_id <= 0
            || frame.date <= 0
            || frame.chat_type != "private"
        {
            Err(NativeIdentityError::Unsupported)
        } else {
            frame
                .actor_id
                .map(|actor| ExcursionKey {
                    connector_id: frame.connector.to_string(),
                    bot_id: frame.bot_id.to_string(),
                    chat_id: frame.chat_id.to_string(),
                    actor_id: actor.to_string(),
                })
                .ok_or(NativeIdentityError::Denied)
        },
        position: NativePosition {
            date: frame.date,
            update_id: i64::from(frame.update_id),
        },
        message_id: i64::from(frame.message_id),
        alias: request_alias(frame),
    }
}
pub(crate) fn wire_scope(scope: OutputScope) -> TelegramOutputScope {
    TelegramOutputScope {
        scope: IngressScope {
            key: scope.scope.key,
            epoch: scope.scope.epoch,
            ephemeral: scope.scope.ephemeral,
            cleanup_scope: scope.scope.cleanup_scope,
        },
        attempt_id: scope.attempt_id,
    }
}
pub(super) fn output_scope(
    context: &SessionContext,
    attempt: Option<String>,
) -> TelegramOutputScope {
    wire_scope(domain_scope(context, attempt))
}

struct NativeFeedback(TelegramFeedback);
impl FeedbackToken for NativeFeedback {
    fn id(&self) -> &str {
        self.0.id()
    }
    fn close(&self) {
        self.0.close();
    }
    fn is_open(&self) -> bool {
        self.0.is_open()
    }
    fn attach(&self, output: Envelope) -> Envelope {
        output.with_context(self.0.clone())
    }
    fn fresh(&self, id: &str) -> TurnFeedback {
        TurnFeedback::from_token(Self(TelegramFeedback::new(id)))
    }
}
pub struct TelegramTransport;
impl HostTransport for TelegramTransport {
    fn ingress(&self, envelope: Arc<Envelope>) -> Arc<Envelope> {
        let Some(batch) = envelope.context.get::<NativeBatch>() else {
            return envelope;
        };
        let constituents = batch
            .frames
            .iter()
            .map(|frame| NativeConstituent {
                connector: frame.connector.clone(),
                channel: frame.chat_id.to_string(),
                alias: request_alias(frame),
                source: json!({
                    "bot_id": frame.bot_id,
                    "connector": frame.connector.as_str(),
                    "chat_id": frame.chat_id,
                    "actor_id": frame.actor_id,
                    "update_id": frame.update_id,
                    "message_id": frame.message_id,
                    "date": frame.date,
                    "chat_type": frame.chat_type,
                }),
            })
            .collect();
        let ingress = NativeIngress { constituents };
        Arc::new((*envelope).clone().with_context(ingress))
    }
    fn feedback(&self, id: &str) -> TurnFeedback {
        TurnFeedback::from_token(NativeFeedback(TelegramFeedback::new(id)))
    }
}

#[cfg(test)]
mod tests {
    use octo_connector_telegram::NativeFrame;

    use super::request_alias;
    fn frame(bot_id: u64, date: i64) -> NativeFrame {
        NativeFrame {
            bot_id,
            connector: ConnectorId::new("telegram"),
            update_id: 9,
            message_id: 7,
            date,
            chat_id: 7,
            actor_id: Some(7),
            chat_type: "private".into(),
            command_text: None,
            digest: "semantic-hash".into(),
            media: false,
        }
    }

    #[test]
    fn reset_update_id_and_another_bot_cannot_alias_an_old_message() {
        let first = request_alias(&frame(1, 1_000_000));
        let later = request_alias(&frame(1, 2_000_000));
        let another = request_alias(&frame(2, 1_000_000));
        assert_ne!(first.idempotency_key, later.idempotency_key);
        assert_ne!(first.namespace, another.namespace);
    }

    use albert_core::{sessions::transport::HostTransport, sessions::transport::TurnFeedback};
    use octo_connector_telegram::TurnFeedback as TelegramFeedback;
    use octo_core::{ConnectorId, Envelope, EventKind};

    use super::TelegramTransport;

    #[test]
    fn native_and_domain_feedback_share_closure_but_resumed_presentations_do_not() {
        let token = TelegramTransport.feedback("initial");
        let output = token.output(Envelope::new(
            ConnectorId::new("albert"),
            EventKind::new("chat.status"),
            String::new(),
        ));
        let queued = output.context.get::<TelegramFeedback>().unwrap();
        let resumed = token.fresh("resumed");
        token.close();
        assert!(!queued.is_open());
        assert!(!output.context.get::<TurnFeedback>().unwrap().is_open());
        assert!(resumed.is_open());
        assert_eq!(resumed.id(), "resumed");

        let resumed_output = resumed.output(output);
        let native = resumed_output.context.get::<TelegramFeedback>().unwrap();
        native.close();
        assert!(!resumed.is_open());
        assert_eq!(native.id(), "resumed");
    }
}
