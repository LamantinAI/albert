//! Restart classifications are separate from work and transport completion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecoveryAction {
    RecheckQueued,
    InterruptedUnknown,
    ContinueDelivery,
    RetryKnownFailure,
    TransportUnknown,
    Terminal,
}
#[derive(Clone, Debug)]
pub struct RecoveryItem {
    pub message_id: String,
    pub action: RecoveryAction,
}
