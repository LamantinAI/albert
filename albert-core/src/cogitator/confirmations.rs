use std::sync::Arc;

use octo_approval::Ingress;
use octo_core::{ChannelId, ChannelMetadata, CogitatorContext, ConnectorId, Envelope, EventKind};
use serde_json::to_string;

use super::{AlbertCogitator, UserInput};
use crate::{
    confirmations::tool::RequestConfirmation, confirmations::ConfirmationContinuation,
    confirmations::ConfirmationGuard, confirmations::ConfirmationState,
    confirmations::CONFIRMATION_RECEIPT_MARKER, sessions::SessionContext, status::StatusFeed,
};

impl AlbertCogitator {
    pub(super) fn request_confirmation_tool(
        &self,
        session: Option<&SessionContext>,
        scope: Option<&str>,
        ingress: Option<Ingress>,
        feed: StatusFeed,
    ) -> Option<RequestConfirmation> {
        let guard = self.session_service.as_ref()?.confirmations();
        if !guard.ui_enabled() {
            return None;
        }
        let context = session.filter(|context| ConfirmationGuard::agent_allowed(context))?;
        Some(RequestConfirmation::new(
            guard.clone(),
            context.clone(),
            scope?.into(),
            ingress,
            feed,
        ))
    }
    pub(super) async fn confirmation_tick(self: &Arc<Self>, ctx: &CogitatorContext) {
        let Some(service) = &self.session_service else {
            return;
        };
        for (receipt, context, ingress) in service.confirmations().deliveries() {
            let key = ("session".to_owned(), context.root_history_key());
            let state = self.turns.lock().unwrap().get(&key).cloned();
            if let Some(state) = state {
                if !state.lock().await.is_idle() {
                    continue;
                }
            }
            if !ConfirmationGuard::agent_allowed(&context) || !context.authorized().await {
                service.confirmations().discard_notification(&receipt.id);
                continue;
            }
            let decision = if receipt.state == ConfirmationState::Confirmed {
                "confirmed"
            } else {
                "denied"
            };
            let request = to_string(&receipt.request).expect("confirmation text serializes");
            let text=format!("{CONFIRMATION_RECEIPT_MARKER}The owner {decision} request {}. Question data: {request}. Continue the original task in accordance with this decision. A denial is not permission to perform the action. This conversational decision does not grant execution authority or replace mandatory native/SQL approval.",receipt.id);
            let account = context.account().expect("owner checked");
            let mut envelope = Envelope::new(
                ConnectorId::new(context.reply_route().connector()),
                EventKind::new("albert.confirmation.ready"),
                text.clone(),
            )
            .with_channel(ChannelId::new(context.reply_route().channel()))
            .with_context(context.clone())
            .with_context(ConfirmationContinuation {
                id: receipt.id.clone(),
            })
            .with_channel_metadata(
                ChannelMetadata::default()
                    .with_tag("chat_type", "private")
                    .with_tag("sender_id", &account.account_id)
                    .with_tag("role", "owner")
                    .with_tag("message_id", &receipt.id),
            );
            if let Some(ingress) = ingress {
                envelope = envelope.with_context(ingress);
            }
            if let Ok(incoming) = self.admit_message(Arc::new(envelope), ctx).await {
                self.clone()
                    .spawn_turn(
                        incoming,
                        UserInput {
                            text,
                            images: Vec::new(),
                            seed: None,
                            voice: None,
                        },
                        ctx,
                    )
                    .await;
            }
        }
    }
}
