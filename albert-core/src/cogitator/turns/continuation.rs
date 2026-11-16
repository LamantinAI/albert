use octo_core::Envelope;

use crate::{
    confirmations::ConfirmationContinuation, credentials::CredentialContinuation,
    service::session::SessionService,
};

pub(super) enum TurnContinuation {
    Credential(String),
    Confirmation(String),
}
impl TurnContinuation {
    pub fn from_envelope(incoming: &Envelope) -> Option<Self> {
        if let Some(receipt) = incoming.context.get::<CredentialContinuation>() {
            return Some(Self::Credential(receipt.id.clone()));
        }
        incoming
            .context
            .get::<ConfirmationContinuation>()
            .map(|receipt| Self::Confirmation(receipt.id.clone()))
    }
    pub fn pending(&self, service: &SessionService) -> bool {
        match self {
            Self::Credential(id) => service.credentials().notification_pending(id),
            Self::Confirmation(id) => service.confirmations().notification_pending(id),
        }
    }
    pub fn schedule(self, service: &SessionService, start: impl FnOnce()) -> bool {
        match self {
            Self::Credential(id) => service.credentials().schedule_notification(&id, start),
            Self::Confirmation(id) => service.confirmations().schedule_notification(&id, start),
        }
    }
}
