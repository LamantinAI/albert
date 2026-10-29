use octo_core::{ConnectorId, Envelope, EventKind};
use octo_history::{SessionId, SessionKind, SessionUpdate as MetadataUpdate};

use super::{session_error, view, SessionHost};
use crate::{
    api::{
        errors::{ApiError, ApiResult},
        host::HostResponse,
        models::SessionUpdate,
    },
    service::session::web::WebActor,
    sessions::SessionEnded,
};

impl SessionHost {
    pub(super) async fn update(
        &self,
        actor: &WebActor,
        id: &str,
        input: SessionUpdate,
    ) -> ApiResult<HostResponse> {
        let id = SessionId::parse(id).map_err(|_| ApiError::invalid("Invalid Session ID."))?;
        let _guard = self.gate(&id).await;
        let context = self
            .runtime
            .sessions
            .web_context(actor, &id)
            .await
            .map_err(session_error)?;
        if input.kind.is_some() && input.state.is_some() {
            return Err(ApiError::invalid("Choose one lifecycle transition."));
        }
        if input.kind.as_deref() == Some("permanent")
            && context.session().kind == SessionKind::Permanent
        {
            return Ok(HostResponse::Session(view(context.session())));
        }
        if context.session().revision != input.revision {
            return Err(ApiError::conflict(
                "Chat metadata changed; reload before retrying.",
            ));
        }
        if let Some(kind) = input.kind {
            if kind != "permanent"
                || input
                    .title
                    .as_ref()
                    .is_some_and(|title| title != &context.session().title)
                || input.archived.is_some()
            {
                return Err(ApiError::invalid(
                    "Save changes only temporary retention; rename separately.",
                ));
            }
            let key = input
                .promotion_key
                .filter(|key| !key.is_empty() && key.len() <= 256)
                .ok_or_else(|| ApiError::invalid("A stable promotion_key is required."))?;
            let saved = self
                .runtime
                .sessions
                .save_temporary(&context, &key, None)
                .await
                .map_err(session_error)?;
            self.hub.changed(
                saved.session_id(),
                actor.user_id.as_str(),
                &self.runtime.status().runtime_epoch,
                "changed",
                None,
            );
            return Ok(HostResponse::Session(view(saved.session())));
        }
        if let Some(state) = input.state {
            if state != "ended" || input.title.is_some() || input.archived.is_some() {
                return Err(ApiError::invalid("Invalid End transition."));
            }
            let (session, discarded) = self
                .runtime
                .sessions
                .web_end(actor, &id, input.revision)
                .await
                .map_err(session_error)?;
            let mut result = view(&session);
            if discarded {
                // Wake existing subscribers so their fenced context closes the
                // stream even when the chat has no further runtime activity.
                self.hub.changed(
                    &id,
                    actor.user_id.as_str(),
                    &self.runtime.status().runtime_epoch,
                    "ended",
                    None,
                );
                self.hub.remove(&id);
                let io = self.context.read().unwrap().clone();
                if let Some(context) = io {
                    let _ = context
                        .publish(
                            Envelope::new(
                                ConnectorId::new("web"),
                                EventKind::new("albert.session.closed"),
                                (),
                            )
                            .with_context(SessionEnded {
                                session_id: id.clone(),
                            }),
                        )
                        .await;
                }
                result.state = "ended".into();
            }
            return Ok(HostResponse::Session(result));
        }
        let session = self
            .runtime
            .sessions
            .web_update(
                actor,
                &id,
                MetadataUpdate {
                    expected_revision: input.revision,
                    title: input.title,
                    archived: input.archived,
                },
            )
            .await
            .map_err(session_error)?;
        self.hub.changed(
            &id,
            actor.user_id.as_str(),
            &self.runtime.status().runtime_epoch,
            "changed",
            None,
        );
        Ok(HostResponse::Session(view(&session)))
    }
}
