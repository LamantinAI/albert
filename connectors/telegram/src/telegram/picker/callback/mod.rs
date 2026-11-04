use std::time::Instant;

use albert_core::{
    sessions::error::SessionError, sessions::error::SessionResult, sessions::ExternalIdentity,
    sessions::ReplyRoute,
};
use octo_connector_telegram::{CallbackDecision, CallbackFrame};
use octo_core::Envelope;

use super::{registry::Action, registry::Reserved, registry::Ticket, registry::EXPIRED};
use crate::{
    telegram::navigation::navigation_notice, telegram::navigation::transition_events,
    telegram::policy::SessionTelegramPolicy,
};

impl SessionTelegramPolicy {
    pub(in crate::telegram) async fn picker_callback(
        &self,
        frame: CallbackFrame,
    ) -> Result<CallbackDecision, String> {
        // Reserve before the first await, including waiting for native admission.
        let reserved = self
            .picker
            .registry
            .lock()
            .map_err(|_| "Session controls are unavailable.".to_owned())?
            .reserve(&frame, Instant::now());
        let ticket = match reserved {
            Ok(Reserved::New(ticket)) => ticket,
            Ok(Reserved::Replay(ack)) => return CallbackDecision::new(Some(ack), false, vec![]),
            Err(message) => return CallbackDecision::new(Some(message.into()), false, vec![]),
        };
        let _guard = self.admission.lock().await;
        let result = self.apply_picker(&frame, &ticket).await;
        let (ack, events) = match result {
            Ok(events) => ("Chat controls updated.".to_owned(), events),
            Err(error) => (error.to_string(), vec![]),
        };
        // Error text comes only from host policy; bound ACK without truncating names.
        let ack = if ack.chars().count() > 200 {
            "Could not update this chat. Use /sessions for a fresh list.".into()
        } else {
            ack
        };
        self.picker
            .registry
            .lock()
            .map_err(|_| "Session controls are unavailable.".to_owned())?
            .finish(&ticket, &ack);
        CallbackDecision::new(Some(ack), false, events)
    }
    async fn apply_picker(
        &self,
        frame: &CallbackFrame,
        ticket: &Ticket,
    ) -> SessionResult<Vec<Envelope>> {
        let message = frame.message.as_ref().expect("reserved private message");
        let identity =
            ExternalIdentity::new(frame.connector.as_str(), &frame.actor_id.to_string())?;
        let reply = ReplyRoute::new(frame.connector.as_str(), &message.chat_id.to_string(), None)?;
        let mut context = self
            .service
            .picker_context(identity, reply, &ticket.binding.user)
            .await?;
        if let Some(accounts) = &self.accounts {
            let actor = i64::try_from(frame.actor_id).map_err(|_| SessionError::Denied)?;
            let grant = accounts
                .actor(frame.connector.as_str(), frame.bot_id, actor)
                .await
                .map_err(|_| SessionError::Denied)?
                .ok_or(SessionError::Denied)?;
            if context
                .user_id()
                .is_none_or(|user| user.as_str() != grant.user_id)
                || !grant.valid().await
            {
                return Err(SessionError::Denied);
            }
            context = context.with_account(grant);
        }
        if !self
            .picker
            .registry
            .lock()
            .map_err(|_| SessionError::Unavailable)?
            .pending(ticket, Instant::now())
        {
            return Err(SessionError::InvalidInput(EXPIRED));
        }
        match &ticket.action {
            Action::Open(id) => {
                let selected = self
                    .service
                    .open_private(
                        &context,
                        id,
                        context
                            .selector_revision()
                            .ok_or(SessionError::Unavailable)?,
                    )
                    .await?;
                let text = self.service.open_notice(&selected).await;
                let mut events = transition_events(&self.service, &context, &selected, true);
                events.push(navigation_notice(&selected, text));
                Ok(events)
            }
            Action::Page(cursor) => Ok(vec![
                self.picker_page(&context, ticket.binding.clone(), cursor.clone())
                    .await?,
            ]),
        }
    }
}
