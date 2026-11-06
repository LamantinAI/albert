use std::time::Duration;

use tokio::time::timeout;

use albert_core::{
    cogitator::source_context, composer::message_digest, history::Turn, media::encode_selected,
    service::session::web::start::StartRecord, service::session::web::WebActor,
    sessions::acceptance::AcceptedStart, sessions::actor::ActorToolPolicy,
};
use octo_history::{AppendRequest, SessionId};
use serde_json::to_vec;
use sha2::{Digest, Sha256};

use super::{
    composer::selections, media::valid_message, send::input_envelope, send::namespace,
    send::InputExtras, session_error, SessionHost,
};
use crate::{
    api::errors::ApiError, api::errors::ApiResult, api::models::SessionStart,
    api::models::SessionStartView,
};

fn view(record: StartRecord) -> SessionStartView {
    let published = record.published();
    SessionStartView {
        id: record.request,
        session_id: record.session,
        state: record.phase,
        published,
        temporary: record.temporary,
    }
}
impl SessionHost {
    pub(super) async fn get_start(
        &self,
        actor: &WebActor,
        id: &str,
    ) -> ApiResult<SessionStartView> {
        let id = SessionId::parse(id)
            .map_err(|_| ApiError::invalid("Invalid start ID."))?
            .to_string();
        let row = self
            .runtime
            .sessions
            .web_start_record(actor, &id)
            .await
            .map_err(session_error)?
            .ok_or_else(ApiError::not_found)?;
        self.runtime
            .sessions
            .web_reconcile_start(actor, &row)
            .await
            .map_err(session_error)?;
        Ok(view(
            self.runtime
                .sessions
                .web_start_record(actor, &id)
                .await
                .map_err(session_error)?
                .ok_or_else(ApiError::not_found)?,
        ))
    }
    pub(super) async fn start(
        &self,
        actor: &WebActor,
        input: SessionStart,
    ) -> ApiResult<SessionStartView> {
        let id = SessionId::parse(&input.client_message_id)
            .map_err(|_| ApiError::invalid("client_message_id must be a UUID."))?;
        let _gate = self.gate(&id).await;
        valid_message(&input.text, &input.attachments)?;
        let selections = selections(&input.skills);
        let input_digest = message_digest(&input.text, &input.attachments, &selections);
        let digest = format!(
            "{:x}",
            Sha256::digest(
                if !selections.is_empty() {
                    to_vec(&(
                        &input.text,
                        input.temporary,
                        &input.attachments,
                        &selections,
                    ))
                } else if input.attachments.is_empty() {
                    to_vec(&(&input.text, input.temporary))
                } else {
                    to_vec(&(&input.text, input.temporary, &input.attachments))
                }
                .expect("input serializes")
            )
        );
        let mut row = if let Some(existing) = self
            .runtime
            .sessions
            .web_start_record(actor, id.as_str())
            .await
            .map_err(session_error)?
        {
            if existing.digest != digest {
                return Err(ApiError::conflict(
                    "Start ID is already bound to another message or retention choice.",
                ));
            }
            existing
        } else {
            if self.runtime.current().is_none()
                || !self.runtime.status().model_ready
                || self.context.read().unwrap().is_none()
            {
                return Err(ApiError::unavailable(
                    "Connect a model before sending messages.",
                ));
            }
            let selected = self.prepare_skills(actor, &selections)?;
            let media = self
                .prepare_media(actor, None, &input.text, &input.attachments)
                .await?;
            let envelope = input_envelope(actor, input.text.clone(), id.as_str());
            let source = source_context(&envelope);
            let transcript = if source.is_empty() {
                input.text.clone()
            } else {
                format!("{source}\n{}", input.text)
            };
            let transcript = encode_selected(transcript, media.assets, selected.stamps());
            self.runtime
                .sessions
                .web_start_create(
                    actor,
                    id.as_str(),
                    &digest,
                    input.temporary,
                    AppendRequest {
                        namespace: namespace(actor),
                        idempotency_key: id.to_string(),
                        digest: input_digest.clone(),
                        turns: vec![Turn::user(transcript)],
                    },
                )
                .await
                .map_err(session_error)?
        };
        if row.phase == "accepted" && row.account != actor.account_id {
            return Err(ApiError::conflict(
                "Resume this first request using the sign-in that accepted it.",
            ));
        }
        if row.phase == "accepted"
            && row.host_tools != (actor.tools == ActorToolPolicy::ExistingHost)
        {
            self.runtime.sessions.web_fail_start(actor,&row,"Permissions changed before this request started. Submit a new request to continue.").await.map_err(session_error)?;
            return self.get_start(actor, id.as_str()).await;
        }
        if self.runtime.current().is_some()
            && self.runtime.status().model_ready
            && row.phase == "accepted"
        {
            let skills = match self.prepare_skills(actor, &selections) {
                Ok(skills) => skills,
                Err(_) => {
                    self.runtime.sessions.web_fail_start(actor,&row,"Selected skills changed before this request started. Select them again in a new request.").await.map_err(session_error)?;
                    return self.get_start(actor, id.as_str()).await;
                }
            };
            let media = match self
                .prepare_media(actor, Some(&row.session), &input.text, &input.attachments)
                .await
            {
                Ok(media) => media,
                Err(_) => {
                    self.runtime.sessions.web_fail_start(actor,&row,"The accepted attachments could not be loaded. Start another message to continue.").await.map_err(session_error)?;
                    return self.get_start(actor, id.as_str()).await;
                }
            };
            if self
                .runtime
                .sessions
                .web_claim_start(actor, id.as_str())
                .await
                .map_err(session_error)?
            {
                row.phase = "claimed".into();
                let submitted: ApiResult<()> = async {
                    let context = self.resource(actor, &row.session).await?;
                    let proof = AcceptedStart::new(
                        row.session.clone(),
                        id.to_string(),
                        input_digest.clone(),
                    );
                    let receipt = self
                        .dispatch_input(
                            actor,
                            &context,
                            input.text,
                            id.as_str(),
                            Some(proof),
                            Some(media),
                            InputExtras {
                                skills,
                                digest: Some(input_digest),
                                compact: None,
                            },
                        )
                        .await?;
                    match timeout(Duration::from_secs(10), receipt).await {
                        Ok(Ok(Ok(_))) => Ok(()),
                        _ => Err(ApiError::unavailable(
                            "First message admission could not be confirmed.",
                        )),
                    }
                }
                .await;
                if submitted.is_err() {
                    self.runtime.sessions.web_fail_start(actor,&row,"Delivery of this first request could not be confirmed. Check recorded actions before retrying.").await.map_err(session_error)?;
                }
            }
        }
        self.get_start(actor, id.as_str()).await
    }
    pub(super) async fn cancel_start(
        &self,
        actor: &WebActor,
        id: &str,
    ) -> ApiResult<SessionStartView> {
        let row = self
            .runtime
            .sessions
            .web_start_record(actor, id)
            .await
            .map_err(session_error)?
            .ok_or_else(ApiError::not_found)?;
        if !row.published() {
            let session = SessionId::parse(&row.session).map_err(|_| ApiError::not_found())?;
            if let (Some(agent), Some(ctx)) = (self.runtime.current(), self.runtime.context()) {
                if let Some(attempt) = agent.panel_active(&session).await {
                    agent.panel_cancel_exact(&session, &attempt, &ctx).await;
                }
            }
            self.runtime
                .sessions
                .web_fail_start(
                    actor,
                    &row,
                    "This first run was stopped. External actions already performed may remain.",
                )
                .await
                .map_err(session_error)?;
        }
        self.get_start(actor, id).await
    }
}
