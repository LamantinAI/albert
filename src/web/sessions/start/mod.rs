use octo_history::{AppendRequest, SessionId};
use serde_json::to_vec;
use sha2::{Digest, Sha256};

use super::{
    send::{input_envelope, namespace},
    session_error, SessionHost,
};
use crate::{
    api::{
        errors::{ApiError, ApiResult},
        models::{SessionStart, SessionStartView},
    },
    cogitator::source_context,
    history::Turn,
    service::session::web::{start::StartRecord, WebActor},
    sessions::{acceptance::AcceptedStart, actor::ActorToolPolicy},
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
        if input.text.trim().is_empty() || input.text.len() > 1024 * 1024 {
            return Err(ApiError::invalid(
                "Message text must contain 1–1048576 bytes.",
            ));
        }
        let digest = format!(
            "{:x}",
            Sha256::digest(to_vec(&(&input.text, input.temporary)).expect("input serializes"))
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
            let envelope = input_envelope(actor, input.text.clone(), id.as_str());
            let source = source_context(&envelope);
            let transcript = if source.is_empty() {
                input.text.clone()
            } else {
                format!("{source}\n{}", input.text)
            };
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
                        digest: format!("{:x}", Sha256::digest(input.text.as_bytes())),
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
            && self
                .runtime
                .sessions
                .web_claim_start(actor, id.as_str())
                .await
                .map_err(session_error)?
        {
            row.phase = "claimed".into();
            let context = self.resource(actor, &row.session).await?;
            let proof = AcceptedStart::new(
                row.session.clone(),
                id.to_string(),
                format!("{:x}", Sha256::digest(input.text.as_bytes())),
            );
            if self
                .dispatch_input(actor, &context, input.text, id.as_str(), Some(proof))
                .await
                .is_err()
            {
                self.runtime.sessions.web_fail_start(actor,&row,"Delivery of this first request could not be confirmed. Check recorded actions before retrying.").await.map_err(session_error)?;
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
