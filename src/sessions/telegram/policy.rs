use std::{
    collections::HashSet,
    sync::{Arc, Mutex as SyncMutex},
    time::Duration,
};

use async_trait::async_trait;
use octo_approval::{Authority, Ingress, ScopeBinding};
use octo_connector_telegram::{
    IngressDecision, IngressScope, NativeFrame, TelegramHostPolicy, TelegramOutputScope,
    TrackingLimits,
};
use octo_core::Envelope;
use tokio::sync::Mutex;

use super::{commands::control, output_scope};
use crate::{
    commands::parse,
    service::session::{NativeRoute, SessionService},
    sessions::{
        error::{SessionError, SessionResult},
        native::request_alias,
        Admission, AdmissionScope, ExternalIdentity, ReplyRoute, SessionContext,
    },
};

/// Invoked after the source adapter authenticates the sender and applies ACLs.
pub(crate) struct SessionTelegramPolicy {
    pub(super) service: Arc<SessionService>,
    pub(super) authority: Authority,
    admission: Mutex<()>,
    pub(super) saved_ended: SyncMutex<HashSet<(String, String)>>,
}

impl SessionTelegramPolicy {
    pub fn new(service: Arc<SessionService>, authority: Authority) -> Self {
        Self {
            service,
            authority,
            admission: Mutex::new(()),
            saved_ended: SyncMutex::new(HashSet::new()),
        }
    }

    async fn admit_inner(
        &self,
        frame: NativeFrame,
        provisional: Envelope,
    ) -> SessionResult<IngressDecision> {
        let admission = verified_admission(&frame, &provisional)?;
        let alias = request_alias(&frame);
        if let Some(binding) = self
            .service
            .journal_router()
            .find_request(&alias.namespace, &alias.idempotency_key)
            .await?
        {
            if binding.digest != alias.digest {
                return Err(SessionError::InvalidInput(
                    "native message conflicts with its accepted content",
                ));
            }
            return Ok(IngressDecision::Ignore);
        }
        let command = frame
            .command_text
            .as_deref()
            .and_then(parse)
            .filter(|command| {
                matches!(
                    command.name.as_str(),
                    "sessions" | "new" | "open" | "tmp" | "save" | "end"
                )
            });
        let native_route = if admission.scope == AdmissionScope::Private {
            self.service.resolve_native_frame(&frame).await?
        } else {
            NativeRoute::Current
        };
        if matches!(native_route, NativeRoute::ControlReplay) {
            return Ok(IngressDecision::Ignore);
        }
        if let Some(command) = command {
            if admission.scope != AdmissionScope::Private {
                return Err(SessionError::GroupSelector);
            }
            if let NativeRoute::Pinned { closed: true, .. } = &native_route {
                return Ok(IngressDecision::Ignore);
            }
            let context = match &native_route {
                NativeRoute::Pinned {
                    session_id,
                    discarded: false,
                    ..
                } => {
                    self.service
                        .context_for_message(admission.clone(), session_id)
                        .await?
                }
                NativeRoute::Pinned {
                    discarded: true, ..
                } if !matches!(
                    command.name.as_str(),
                    "end" | "tmp" | "open" | "new" | "sessions"
                ) =>
                {
                    return Err(SessionError::Expired);
                }
                _ => self.service.control_context(admission.clone()).await?,
            };
            // Delayed controls must not change a newer navigation interval.
            let current = self.service.control_context(admission).await?;
            if context.session_id() != current.session_id() {
                return Ok(IngressDecision::Ignore);
            }
            return control(self, &frame, context, &command.name, command.args).await;
        }
        let context = match native_route {
            NativeRoute::Pinned {
                discarded: true, ..
            } => return Err(SessionError::Ended),
            NativeRoute::Pinned { session_id, .. } => {
                self.service
                    .context_for_message(admission, &session_id)
                    .await?
            }
            NativeRoute::Current => self.service.admit(admission).await?,
            NativeRoute::ControlReplay => return Ok(IngressDecision::Ignore),
        };
        self.service.note_native_frame(&context, &frame)?;
        let scope = output_scope(&context, None).scope;
        Ok(IngressDecision::Accept {
            envelope: self.stamp(provisional, context)?,
            scope,
        })
    }

    pub(super) fn stamp(
        &self,
        envelope: Envelope,
        context: SessionContext,
    ) -> SessionResult<Envelope> {
        let mut envelope = envelope;
        if let (Some(epoch), Some(ingress)) =
            (context.scope_epoch(), envelope.context.get::<Ingress>())
        {
            let scope = ScopeBinding::new(context.root_history_key(), epoch.to_owned())
                .map_err(|_| SessionError::Denied)?;
            let proof = self
                .authority
                .bind_scope(ingress, scope)
                .map_err(|_| SessionError::Denied)?;
            envelope.context.insert(proof);
        }
        Ok(envelope.with_context(context))
    }
}

#[async_trait]
impl TelegramHostPolicy for SessionTelegramPolicy {
    async fn admit(
        &self,
        frame: NativeFrame,
        provisional: Envelope,
    ) -> Result<IngressDecision, String> {
        let _guard = self.admission.lock().await;
        Ok(match self.admit_inner(frame, provisional).await {
            Ok(decision) => decision,
            Err(error) => IngressDecision::Control {
                notice: Some(error.to_string()),
                scope: None,
                post_events: Vec::new(),
                cleanup: None,
            },
        })
    }
    fn allow_publish(&self, envelope: &Envelope) -> bool {
        live(envelope)
    }
    fn allow_delivery(&self, envelope: &Envelope) -> bool {
        live(envelope)
    }
    fn tracking_limits(&self) -> TrackingLimits {
        let limits = &self.service.settings().limits;
        TrackingLimits {
            max_scopes: limits.telegram_tracking_max_scopes,
            max_ids_per_scope: limits.telegram_tracking_max_ids,
            cleanup_batches: limits.telegram_cleanup_batches,
            cleanup_timeout: Duration::from_millis(limits.telegram_cleanup_timeout_ms),
        }
    }
    fn output_scope(&self, envelope: &Envelope) -> Option<TelegramOutputScope> {
        let mut output = envelope
            .context
            .get::<TelegramOutputScope>()
            .cloned()
            .or_else(|| {
                envelope
                    .context
                    .get::<IngressScope>()
                    .cloned()
                    .map(|scope| TelegramOutputScope {
                        scope,
                        attempt_id: None,
                    })
            })?;
        if let Some(cleanup) = &output.scope.cleanup_scope {
            if self
                .saved_ended
                .lock()
                .ok()
                .is_some_and(|ended| ended.contains(&(cleanup.clone(), output.scope.epoch.clone())))
            {
                output.scope.ephemeral = false;
                output.scope.cleanup_scope = None;
            }
        }
        Some(output)
    }
}

fn live(envelope: &Envelope) -> bool {
    envelope
        .context
        .get::<SessionContext>()
        .is_none_or(SessionContext::is_live)
}

fn verified_admission(frame: &NativeFrame, envelope: &Envelope) -> SessionResult<Admission> {
    if envelope.kind.as_str() != "chat.message"
        || envelope.source != frame.connector
        || envelope
            .channel
            .as_ref()
            .map(|channel| channel.as_str().to_owned())
            != Some(frame.chat_id.to_string())
        || frame.bot_id == 0
        || frame.date <= 0
        || frame.message_id <= 0
    {
        return Err(SessionError::Denied);
    }
    let route = ReplyRoute::new(
        frame.connector.as_str(),
        &frame.chat_id.to_string(),
        Some(envelope.id.to_string()),
    )?;
    let identity = frame
        .actor_id
        .map(|actor| ExternalIdentity::new(frame.connector.as_str(), &actor.to_string()))
        .transpose()?;
    match frame.chat_type.as_str() {
        "private" => Admission::new(
            identity.ok_or(SessionError::Denied)?,
            route,
            AdmissionScope::Private,
            "host",
        ),
        "group" | "supergroup" => Admission::group(identity, route, "host"),
        _ => Err(SessionError::InvalidInput(
            "this Telegram channel does not support sessions",
        )),
    }
}
