use std::sync::Arc;

use octo_approval::Ingress;
use octo_core::{ChannelId, ChannelMetadata, CogitatorContext, ConnectorId, Envelope, EventKind};

use super::{AlbertCogitator, UserInput};
use crate::{
    credentials::{tool::RequestCredential, CredentialContinuation, CredentialGuard},
    sessions::SessionContext,
};

impl AlbertCogitator {
    pub(super) fn request_credential_tool(
        &self,
        session: Option<&SessionContext>,
        scope: Option<&str>,
        ingress: Option<Ingress>,
    ) -> Option<RequestCredential> {
        if !self.session_service.as_ref()?.credentials().ui_enabled() {
            return None;
        }
        let context = session.filter(|context| CredentialGuard::agent_allowed(context))?;
        Some(RequestCredential::new(
            self.session_service.as_ref()?.credentials().clone(),
            context.clone(),
            scope?.into(),
            ingress,
        ))
    }
    pub(super) async fn credential_tick(self: &Arc<Self>, ctx: &CogitatorContext) {
        let Some(service) = &self.session_service else {
            return;
        };
        service.record_manual_credentials().await;
        for (receipt, context, ingress) in service.credentials().deliveries() {
            let key = ("session".to_owned(), context.root_history_key());
            let state = self.turns.lock().unwrap().get(&key).cloned();
            if let Some(state) = state {
                if !state.lock().await.is_idle() {
                    continue;
                }
            }
            if !CredentialGuard::agent_allowed(&context) || !context.authorized().await {
                service
                    .credentials()
                    .notification_recorded(&receipt.id, false);
                continue;
            }
            let Some(name) = receipt.name.as_deref() else {
                continue;
            };
            let account = context.account().expect("owner checked");
            let channel = context.reply_route().channel();
            let text = receipt_text(name);
            let mut envelope = Envelope::new(
                ConnectorId::new(context.reply_route().connector()),
                EventKind::new("albert.credential.ready"),
                text.clone(),
            )
            .with_channel(ChannelId::new(channel))
            .with_context(context.clone())
            .with_context(CredentialContinuation {
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

pub(super) fn receipt_text(name: &str) -> String {
    format!("[Credential handoff receipt] Server credential {name} was saved by the owner. Its value is not available in this conversation. Continue relevant work using the credential name, without requesting or reading its value. Saving does not itself apply connector configuration.")
}
