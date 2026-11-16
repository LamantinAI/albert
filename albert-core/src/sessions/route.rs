//! The original physical reply destination of an admitted input.
use super::{error::SessionError, error::SessionResult, validate_identifier};
use octo_history::SessionOrigin;
/// A reply always returns to the physical ingress, including after a selector
/// changes. This is separate from both the user identity and model SQL access.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplyRoute {
    pub(super) connector: String,
    pub(super) channel: String,
    pub(super) correlation: Option<String>,
}

impl ReplyRoute {
    pub fn new(connector: &str, channel: &str, correlation: Option<String>) -> SessionResult<Self> {
        validate_identifier(connector)?;
        validate_identifier(channel)?;
        if correlation
            .as_ref()
            .is_some_and(|value| value.len() > 4096 || value.contains('\0'))
        {
            return Err(SessionError::InvalidInput("invalid correlation"));
        }
        Ok(Self {
            connector: connector.into(),
            channel: channel.into(),
            correlation,
        })
    }
    pub fn connector(&self) -> &str {
        &self.connector
    }
    pub fn channel(&self) -> &str {
        &self.channel
    }
    pub fn origin(&self) -> SessionOrigin {
        SessionOrigin {
            connector: self.connector.clone(),
            channel: self.channel.clone(),
        }
    }
}
