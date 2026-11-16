//! Canonical user identity and verified external identity.
use super::{error::SessionResult, validate_identifier};
use sha2::{Digest, Sha256};
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct UserId(pub(super) String);

impl UserId {
    pub fn parse(value: &str) -> SessionResult<Self> {
        validate_identifier(value)?;
        Ok(Self(value.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub(crate) fn for_identity(identity: &ExternalIdentity) -> Self {
        let mut digest = Sha256::new();
        digest.update((identity.connector.len() as u64).to_be_bytes());
        digest.update(identity.connector.as_bytes());
        digest.update(identity.actor.as_bytes());
        Self(format!("user/{:x}", digest.finalize()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalIdentity {
    pub(crate) connector: String,
    pub(crate) actor: String,
}

impl ExternalIdentity {
    pub fn new(connector: &str, actor: &str) -> SessionResult<Self> {
        validate_identifier(connector)?;
        validate_identifier(actor)?;
        Ok(Self {
            connector: connector.into(),
            actor: actor.into(),
        })
    }
    pub fn connector(&self) -> &str {
        &self.connector
    }
    pub fn actor(&self) -> &str {
        &self.actor
    }
}
