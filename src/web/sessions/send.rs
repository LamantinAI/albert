use std::time::Duration;

use octo_approval::{Access, Origin, ScopeBinding};
use octo_core::{ChannelId, ChannelMetadata, ConnectorId, Envelope, EventKind};
use octo_history::{AppendReceipt, RequestBinding, SessionId};
use serde_json::to_vec;
use sha2::{Digest, Sha256};
use tokio::time::timeout;

use super::{session_error, SessionHost};
use crate::{
    api::{
        errors::{ApiError, ApiResult},
        models::{MessageCreate, MessageReceipt},
    },
    service::session::web::WebActor,
    sessions::{acceptance::SourceReceipt, actor::ActorToolPolicy},
};

fn request_id(value: &str) -> ApiResult<String> {
    SessionId::parse(value)
        .map(|id| id.to_string())
        .map_err(|_| ApiError::invalid("client_message_id must be a UUID."))
}
pub(super) fn namespace(actor: &WebActor) -> String {
    let channel = format!("user:{}", actor.user_id.as_str());
    format!(
        "albert/native/{:x}",
        Sha256::digest(
            to_vec(&("web", Some(channel.as_str()))).expect("transport identity serializes")
        )
    )
}
fn response(id: String, receipt: AppendReceipt) -> MessageReceipt {
    MessageReceipt {
        client_message_id: id,
        attempt_id: None,
        message_ids: receipt
            .record_ids
            .into_iter()
            .map(|id| id.to_string())
            .collect(),
        duplicate: receipt.duplicate,
    }
}
fn bound(
    binding: RequestBinding,
    session: &SessionId,
    digest: Option<&str>,
) -> ApiResult<AppendReceipt> {
    if binding.journal.session_id() != session
        || digest.is_some_and(|digest| digest != binding.digest)
    {
        return Err(ApiError::conflict(
            "This client message ID is already bound to another chat or body.",
        ));
    }
    Ok(binding.receipt)
}
impl SessionHost {
    pub(super) async fn receipt(
        &self,
        actor: &WebActor,
        id: &str,
        request: &str,
    ) -> ApiResult<MessageReceipt> {
        let context = self.resource(actor, id).await?;
        let request = request_id(request)?;
        let found = self
            .runtime
            .sessions
            .journal_router()
            .find_request(&namespace(actor), &request)
            .await
            .map_err(|_| ApiError::conflict("Receipt state could not be read."))?
            .ok_or_else(ApiError::not_found)?;
        Ok(response(request, bound(found, context.session_id(), None)?))
    }
    pub(super) async fn send(
        &self,
        actor: &WebActor,
        id: &str,
        input: MessageCreate,
    ) -> ApiResult<MessageReceipt> {
        let session = SessionId::parse(id).map_err(|_| ApiError::invalid("Invalid Session ID."))?;
        let _guard = self.gate(&session).await;
        let context = self.resource(actor, id).await?;
        let request = request_id(&input.client_message_id)?;
        if input.text.trim().is_empty() || input.text.len() > 1024 * 1024 {
            return Err(ApiError::invalid(
                "Message text must contain 1–1048576 bytes.",
            ));
        }
        let digest = format!("{:x}", Sha256::digest(input.text.as_bytes()));
        if input
            .digest
            .as_ref()
            .is_some_and(|provided| provided != &digest)
        {
            return Err(ApiError::conflict(
                "Message digest does not match its body.",
            ));
        }
        let lookup = self.runtime.sessions.journal_router();
        if let Some(binding) = lookup
            .find_request(&namespace(actor), &request)
            .await
            .map_err(|_| ApiError::conflict("Receipt state could not be read."))?
        {
            let mut receipt = bound(binding, &session, Some(&digest))?;
            receipt.duplicate = true;
            return Ok(response(request, receipt));
        }
        if self.runtime.current().is_none() || !self.runtime.status().model_ready {
            return Err(ApiError::unavailable(
                "Connect a model before sending messages.",
            ));
        }
        let io = self
            .context
            .read()
            .unwrap()
            .clone()
            .ok_or_else(|| ApiError::unavailable("Web transport is starting."))?;
        let (accepted, receipt) = SourceReceipt::channel();
        let channel = context.reply_route().channel();
        let owner = actor.tools == ActorToolPolicy::ExistingHost;
        let mut envelope = Envelope::new(
            ConnectorId::new("web"),
            EventKind::new("chat.message"),
            input.text,
        )
        .with_channel(ChannelId::new(channel))
        .with_channel_metadata(
            ChannelMetadata::default()
                .with_tag("chat_type", "private")
                .with_tag("chat_id", channel)
                .with_tag("sender_id", &actor.account_id)
                .with_tag("message_id", &request)
                .with_tag("role", if owner { "owner" } else { "user" }),
        )
        .with_context(context.clone())
        .with_context(accepted);
        if owner {
            let principal = self
                .runtime
                .sessions
                .web_sql_principal(&actor.user_id)
                .await
                .map_err(session_error)?;
            let mut ingress = self.runtime.authority.issue(
                Origin {
                    principal,
                    connector: "web".into(),
                    conversation: channel.into(),
                    private_destination: channel.into(),
                    may_approve: true,
                },
                Access::owner(),
            );
            if let Some(epoch) = context.scope_epoch() {
                ingress = self
                    .runtime
                    .authority
                    .bind_scope(
                        &ingress,
                        ScopeBinding::new(context.root_history_key(), epoch.to_owned())
                            .map_err(|_| ApiError::forbidden())?,
                    )
                    .map_err(|_| ApiError::forbidden())?;
            }
            envelope = envelope.with_context(ingress);
        }
        io.publish(envelope).await.map_err(|_| {
            ApiError::unavailable(
                "Message could not be submitted. Retry with the same client message ID.",
            )
        })?;
        match timeout(Duration::from_secs(10), receipt).await {
            Ok(Ok(Ok(receipt))) => Ok(response(request, receipt)),
            _ => {
                if let Some(binding) = lookup
                    .find_request(&namespace(actor), &request)
                    .await
                    .map_err(|_| {
                        ApiError::unavailable("Acceptance is unknown; retry the same message ID.")
                    })?
                {
                    return Ok(response(request, bound(binding, &session, Some(&digest))?));
                }
                Err(ApiError::unavailable(
                    "Acceptance was not confirmed. Retry with the same client message ID and body.",
                ))
            }
        }
    }
}
