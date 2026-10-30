//! Host-only messaging contexts. Never admit synthetic chat.message, resolve a
//! mutable selector as the recipient identity, or grant native actor privileges.
use octo_history::{SessionId, SessionKind, SessionState};
use sqlx::query_scalar;

use super::SessionService;
use crate::{
    messaging::{
        contract::{DestinationSpec, ReturnAddress},
        MessagingError, MessagingResult,
    },
    sessions::{AdmissionScope, ReplyRoute, SessionContext, UserId},
};

impl SessionService {
    pub async fn messaging_context(
        &self,
        actor: &str,
        recipient: &SessionId,
        return_address: &ReturnAddress,
    ) -> MessagingResult<SessionContext> {
        let user = UserId::parse(actor)?;
        let mut tx = self.history.begin_journal().await?;
        let session = self.catalog_session_in(&mut tx, recipient).await?;
        let lease = self.lease_for_in(&mut tx, recipient).await?;
        let context = SessionContext {
            actor_tools: None,
            lease,
            cleanup_scope: None,
            user: Some(user),
            identity: None,
            reply: ReplyRoute::new(
                &return_address.display.connector,
                &return_address.display.channel,
                None,
            )?,
            scope: AdmissionScope::Private,
            default_id: recipient.clone(),
            session,
            selector_revision: None,
        };
        context.check(context.session())?;
        if context.session().kind == SessionKind::Temporary {
            return Err(MessagingError::SaveRequired);
        }
        if context.session().state != SessionState::Active {
            return Err(MessagingError::Unavailable);
        }
        tx.commit().await?;
        Ok(context)
    }

    /// Diagnose volatile targets through the existing RAM-aware service before
    /// store admission; no temporary text is ever copied into durable routing.
    pub async fn messaging_preflight(
        &self,
        context: &SessionContext,
        destination: &DestinationSpec,
    ) -> MessagingResult<()> {
        self.validate_context(context).await?;
        if context.session().kind == SessionKind::Temporary {
            return Err(MessagingError::SaveRequired);
        }
        let id = match destination {
            DestinationSpec::FixedSession { session, .. } => session.clone(),
            DestinationSpec::ChannelActive { route } => {
                let id: Option<String> = query_scalar("SELECT session_id FROM albert_private_session_routes WHERE connector_id=? AND channel_id=? AND user_id=?")
                    .bind(&route.connector).bind(&route.channel).bind(context.private_user_id()?.as_str())
                    .fetch_optional(self.database.pool()).await?;
                SessionId::parse(&id.ok_or(MessagingError::Unavailable)?)?
            }
        };
        let recipient = self.get(context, &id).await?;
        if recipient.kind == SessionKind::Temporary {
            return Err(MessagingError::SaveRequired);
        }
        if recipient.state != SessionState::Active {
            return Err(MessagingError::Unavailable);
        }
        Ok(())
    }
}
