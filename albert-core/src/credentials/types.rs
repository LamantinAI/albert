use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct CredentialSettings {
    pub request_ttl_seconds: u64,
    pub tombstone_ttl_seconds: u64,
    pub notification_ttl_seconds: u64,
    pub max_pending: usize,
    pub max_entries: usize,
    pub max_value_bytes: usize,
}
impl Default for CredentialSettings {
    fn default() -> Self {
        Self {
            request_ttl_seconds: 900,
            tombstone_ttl_seconds: 300,
            notification_ttl_seconds: 3600,
            max_pending: 64,
            max_entries: 256,
            max_value_bytes: 16 * 1024,
        }
    }
}
impl CredentialSettings {
    pub fn valid(&self) -> bool {
        (30..=86400).contains(&self.request_ttl_seconds)
            && (30..=86400).contains(&self.notification_ttl_seconds)
            && (30..=86400).contains(&self.tombstone_ttl_seconds)
            && (1..=1024).contains(&self.max_pending)
            && (self.max_pending..=4096).contains(&self.max_entries)
            && (1..=1024 * 1024).contains(&self.max_value_bytes)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialState {
    Pending,
    Saving,
    Saved,
    Cancelled,
    Expired,
    Indeterminate,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialSource {
    Agent,
    Manual,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialNotification {
    None,
    Pending,
    Recorded,
    Discarded,
}
/// Secret values never enter request metadata, events or model feedback.
#[derive(Clone, Debug, Serialize)]
pub struct CredentialView {
    pub id: String,
    pub session_id: Option<String>,
    pub suggested_name: String,
    pub name: Option<String>,
    pub source: CredentialSource,
    pub state: CredentialState,
    pub notification: CredentialNotification,
    pub expires_at: i64,
    pub replaced: bool,
    pub replayed: bool,
}
#[derive(Clone, Debug)]
pub struct CredentialChange {
    pub account_id: String,
    pub session_id: Option<String>,
    pub request_id: String,
}
#[derive(Clone, Debug)]
pub struct CredentialContinuation {
    pub id: String,
}
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CredentialError {
    #[error(
        "Credential request not found or no longer available. Its save outcome may be unknown."
    )]
    NotFound,
    #[error("Only the owner may manage server credentials.")]
    Denied,
    #[error(
        "A credential request is already open in this conversation. Complete or cancel it first."
    )]
    Pending,
    #[error("Too many credential requests. Complete an open request or try again later.")]
    Limit,
    #[error("Use letters, digits and underscores for the credential name; start with a letter or underscore.")]
    Name,
    #[error(
        "Credential values must be nonempty single-line text within the configured size limit."
    )]
    Value,
    #[error("This credential request ID belongs to another operation.")]
    Conflict,
    #[error(
        "This credential request has been cancelled or expired. Open a new request to continue."
    )]
    Closed,
}
pub fn valid_credential_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .enumerate()
            .all(|(i, b)| b.is_ascii_alphabetic() || b == b'_' || (i > 0 && b.is_ascii_digit()))
}
