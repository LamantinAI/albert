use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ConfirmationRequest {
    pub title: String,
    pub action: String,
    #[serde(default)]
    pub effects: Vec<String>,
    #[serde(default)]
    pub details: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmationState {
    Pending,
    Confirmed,
    Denied,
    Cancelled,
    Expired,
    Superseded,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Notification {
    None,
    Pending,
    Recorded,
    Discarded,
}
#[derive(Clone, Debug, Serialize)]
pub struct ConfirmationView {
    pub id: String,
    pub session_id: String,
    pub request: ConfirmationRequest,
    pub state: ConfirmationState,
    pub expires_at: i64,
}
#[derive(Clone, Debug)]
pub struct ConfirmationChange {
    pub account_id: String,
    pub session_id: String,
    pub request_id: String,
}
/// Non-serializable host stamp: a transcript or tool argument cannot resume work.
#[derive(Clone, Debug)]
pub struct ConfirmationContinuation {
    pub(crate) id: String,
}
#[derive(Debug, Error)]
pub enum ConfirmationError {
    #[error("Confirmation requires a live private owner conversation with host permissions.")]
    Denied,
    #[error("Confirmation is unavailable.")]
    NotFound,
    #[error("The confirmation already has a different outcome.")]
    Conflict,
    #[error("Confirmation data exceeds the configured limits.")]
    Invalid,
    #[error("Too many pending confirmations. Resolve an earlier request first.")]
    Limit,
}

#[derive(Clone, Debug)]
pub struct ConfirmationWait {
    pub request_id: String,
    pub title: String,
}
impl ConfirmationWait {
    pub fn message(&self) -> String {
        format!(
            "Waiting for your confirmation: {}. Review the approval panel before continuing.",
            self.title
        )
    }
}
