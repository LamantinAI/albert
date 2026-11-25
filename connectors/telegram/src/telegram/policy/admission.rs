use super::{verified_admission, SessionTelegramPolicy};
use crate::telegram::request_alias;
use crate::telegram::{
    accounts::AccountInput, commands::control, navigation as native_navigation, output_scope,
};
use albert_core::{
    commands::parse, service::session::NativeRoute, sessions::error::SessionError,
    sessions::error::SessionResult, sessions::Admission, sessions::AdmissionScope,
    sessions::SessionContext,
};
use octo_approval::Ingress;
use octo_connector_telegram::{IngressDecision, NativeFrame};
use octo_core::Envelope;
impl SessionTelegramPolicy {
    pub(in crate::telegram) async fn admit_inner(
        &self,
        frame: NativeFrame,
        provisional: Envelope,
    ) -> SessionResult<IngressDecision> {
        let admission = verified_admission(&frame, &provisional)?;
        let admission = if let Some(input) = provisional.context.get::<AccountInput>() {
            match &input.grant {
                Some(grant) => admission.with_account(grant.clone())?,
                None => Admission::group(None, admission.route().clone(), "host")?,
            }
        } else {
            admission
        };
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
                    "sessions" | "new" | "open" | "rename" | "tmp" | "save" | "end" | "fork"
                )
            });
        if command
            .as_ref()
            .is_some_and(|command| command.name == "fork")
            && admission.scope() == AdmissionScope::Private
            && self.service.fork_replayed(&alias).await?
        {
            return Ok(IngressDecision::Ignore);
        }
        let native_route = if admission.scope() == AdmissionScope::Private {
            self.service
                .resolve_native_frame(&native_navigation(&frame))
                .await?
        } else {
            NativeRoute::Current
        };
        if matches!(native_route, NativeRoute::ControlReplay) {
            return Ok(IngressDecision::Ignore);
        }
        if let Some(command) = command {
            if admission.scope() != AdmissionScope::Private {
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
        self.service
            .note_native_frame(&context, &native_navigation(&frame))?;
        let scope = output_scope(&context, None).scope;
        Ok(IngressDecision::Accept {
            envelope: self.stamp(provisional, context)?,
            scope,
        })
    }
    pub(in crate::telegram) fn stamp(
        &self,
        envelope: Envelope,
        context: SessionContext,
    ) -> SessionResult<Envelope> {
        let mut envelope = envelope;
        if let (Some(scope), Some(ingress)) = (
            self.service.scope_registry().binding(&context)?,
            envelope.context.get::<Ingress>(),
        ) {
            let proof = self
                .authority
                .bind_scope(ingress, scope)
                .map_err(|_| SessionError::Denied)?;
            envelope.context.insert(proof);
        }
        Ok(envelope.with_context(context))
    }
}
