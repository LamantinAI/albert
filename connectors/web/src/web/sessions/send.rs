use std::time::Duration;

use albert_core::{
    accounts::AccountRole, composer::message_digest, composer::CompactRequest,
    composer::InputDigest, composer::PreparedSkills, media::MediaInput,
    service::session::web::WebActor, sessions::acceptance::AcceptedStart,
    sessions::acceptance::SourceReceipt, sessions::actor::ActorToolPolicy,
    sessions::SessionContext,
};
use octo_approval::{Access, Origin};
use octo_core::{ChannelId, ChannelMetadata, ConnectorId, Envelope, EventKind};
use octo_history::{AppendReceipt, RequestBinding, SessionId};
use serde_json::to_vec;
use sha2::{Digest, Sha256};
use tokio::{sync::oneshot::Receiver, time::timeout};

use super::{composer::selections, media::valid_message, session_error, SessionHost};
use crate::{
    api::errors::ApiError, api::errors::ApiResult, api::models::MessageCreate,
    api::models::MessageReceipt,
};

#[derive(Default)]
pub(super) struct InputExtras {
    pub skills: PreparedSkills,
    pub digest: Option<String>,
    pub compact: Option<CompactRequest>,
}

pub(super) fn request_id(value: &str) -> ApiResult<String> {
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
pub(super) fn response(id: String, receipt: AppendReceipt) -> MessageReceipt {
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
pub(super) fn bound(
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
        valid_message(&input.text, &input.attachments)?;
        let selections = selections(&input.skills);
        let digest = message_digest(&input.text, &input.attachments, &selections);
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
        if self
            .runtime
            .sessions
            .web_waiting_for_admission(&session)
            .await
            .map_err(session_error)?
        {
            return Err(ApiError::conflict(
                "The first message is awaiting admission. Resume it before sending another.",
            ));
        }
        if self.runtime.current().is_none() || !self.runtime.status().model_ready {
            return Err(ApiError::unavailable(
                "Connect a model before sending messages.",
            ));
        }
        let skills = self.prepare_skills(actor, &selections)?;
        let media = self
            .prepare_media(actor, Some(id), &input.text, &input.attachments)
            .await?;
        let receipt = self
            .dispatch_input(
                actor,
                &context,
                input.text,
                &request,
                None,
                Some(media),
                InputExtras {
                    skills,
                    digest: Some(digest.clone()),
                    compact: None,
                },
            )
            .await?;
        match timeout(Duration::from_secs(10), receipt).await {
            Ok(Ok(Ok(receipt))) => Ok(response(request, receipt)),
            Ok(Ok(Err(error))) if error.starts_with("Selected skill admission rejected: ") => {
                Err(ApiError::conflict(error))
            }
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
    pub(super) async fn dispatch_input(
        &self,
        actor: &WebActor,
        context: &SessionContext,
        text: String,
        request: &str,
        proof: Option<AcceptedStart>,
        media: Option<MediaInput>,
        extras: InputExtras,
    ) -> ApiResult<Receiver<Result<AppendReceipt, String>>> {
        let io = self
            .context
            .read()
            .unwrap()
            .clone()
            .ok_or_else(|| ApiError::unavailable("Web transport is starting."))?;
        let grant = self
            .runtime
            .sessions
            .accounts()
            .account(&actor.account_id)
            .await
            .map_err(|_| ApiError::forbidden())?;
        if actor.tools == ActorToolPolicy::ExistingHost && grant.role != AccountRole::Owner {
            return Err(ApiError::forbidden());
        }
        let context = context.clone().with_account(grant);
        if !context.authorized().await {
            return Err(ApiError::forbidden());
        }
        let (accepted, receipt) = SourceReceipt::channel();
        let channel = context.reply_route().channel();
        let owner = actor.tools == ActorToolPolicy::ExistingHost;
        let mut envelope = input_envelope(actor, text, request)
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
            if let Some(binding) = self
                .runtime
                .sessions
                .scope_registry()
                .binding(&context)
                .map_err(session_error)?
            {
                ingress = self
                    .runtime
                    .authority
                    .bind_scope(&ingress, binding)
                    .map_err(|_| ApiError::forbidden())?;
            }
            envelope = envelope.with_context(ingress);
        }
        envelope = envelope.with_context(extras.skills);
        if let Some(digest) = extras.digest {
            envelope = envelope.with_context(InputDigest(digest));
        }
        if let Some(command) = extras.compact {
            envelope = envelope.with_context(command);
        }
        if let Some(media) = media {
            envelope = envelope.with_context(media);
        }
        if let Some(proof) = proof {
            envelope = envelope.with_context(proof);
        }
        io.publish(envelope).await.map_err(|_| {
            ApiError::unavailable(
                "Message could not be submitted. Retry with the same client message ID.",
            )
        })?;
        Ok(receipt)
    }
}

pub(super) fn input_envelope(actor: &WebActor, text: String, request: &str) -> Envelope {
    let channel = format!("user:{}", actor.user_id.as_str());
    Envelope::new(
        ConnectorId::new("web"),
        EventKind::new("chat.message"),
        text,
    )
    .with_channel(ChannelId::new(&channel))
    .with_channel_metadata(
        ChannelMetadata::default()
            .with_tag("chat_type", "private")
            .with_tag("chat_id", &channel)
            .with_tag("sender_id", &actor.account_id)
            .with_tag("message_id", request)
            .with_tag(
                "role",
                if actor.tools == ActorToolPolicy::ExistingHost {
                    "owner"
                } else {
                    "user"
                },
            ),
    )
}
