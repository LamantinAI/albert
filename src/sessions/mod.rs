//! Host-authenticated session policy. These admission types are deliberately not
//! deserializable: an actor or model cannot submit its own owner or reply route.
pub(crate) mod error;
pub(crate) mod ingress;
pub(crate) mod lease;
pub(crate) mod native;
pub(crate) mod settings;
pub(crate) mod telegram;

use std::sync::Arc;

#[cfg(test)]
use octo_history::JournalKeyInventory;
use octo_history::{Session, SessionAccess, SessionId, SessionJournal, SessionOrigin};
use serde_json::to_vec;
use sha2::{Digest, Sha256};

use error::{SessionError, SessionResult};
use lease::SessionLease;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct UserId(String);

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
pub(crate) struct ExternalIdentity {
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

/// A reply always returns to the physical ingress, including after a selector
/// changes. This is separate from both the user identity and model SQL access.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReplyRoute {
    connector: String,
    channel: String,
    correlation: Option<String>,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AdmissionScope {
    Private,
    Group,
}

#[derive(Clone, Debug)]
pub(crate) struct Admission {
    pub(crate) identity: Option<ExternalIdentity>,
    pub(crate) route: ReplyRoute,
    pub(crate) scope: AdmissionScope,
    pub(crate) host: String,
}

impl Admission {
    /// Called only after the source adapter verifies actor and channel scope.
    pub fn new(
        identity: ExternalIdentity,
        route: ReplyRoute,
        scope: AdmissionScope,
        host: &str,
    ) -> SessionResult<Self> {
        if identity.connector != route.connector {
            return Err(SessionError::InvalidInput(
                "identity and ingress connector differ",
            ));
        }
        require_host(host)?;
        Ok(Self {
            identity: Some(identity),
            route,
            scope,
            host: host.into(),
        })
    }
    pub fn group(
        identity: Option<ExternalIdentity>,
        route: ReplyRoute,
        host: &str,
    ) -> SessionResult<Self> {
        if let Some(identity) = identity.as_ref() {
            if identity.connector != route.connector {
                return Err(SessionError::InvalidInput(
                    "identity and ingress connector differ",
                ));
            }
        }
        require_host(host)?;
        Ok(Self {
            identity,
            route,
            scope: AdmissionScope::Group,
            host: host.into(),
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SessionContext {
    pub(crate) lease: Arc<SessionLease>,
    pub(crate) cleanup_scope: Option<String>,
    pub(crate) user: Option<UserId>,
    pub(crate) identity: Option<ExternalIdentity>,
    pub(crate) reply: ReplyRoute,
    pub(crate) scope: AdmissionScope,
    pub(crate) session: Session,
    pub(crate) default_id: SessionId,
    pub(crate) selector_revision: Option<i64>,
}

impl SessionContext {
    pub fn is_live(&self) -> bool {
        self.lease.is_live()
    }
    pub fn scope_epoch(&self) -> Option<&str> {
        self.lease.epoch()
    }
    pub fn cleanup_scope(&self) -> Option<&str> {
        self.cleanup_scope.as_deref()
    }
    pub fn user_id(&self) -> Option<&UserId> {
        self.user.as_ref()
    }
    pub fn identity(&self) -> Option<&ExternalIdentity> {
        self.identity.as_ref()
    }
    pub fn private_user_id(&self) -> SessionResult<&UserId> {
        self.require_private()?;
        self.user.as_ref().ok_or(SessionError::Denied)
    }
    pub fn reply_route(&self) -> &ReplyRoute {
        &self.reply
    }
    pub fn session(&self) -> &Session {
        &self.session
    }
    pub fn session_id(&self) -> &SessionId {
        &self.session.id
    }
    pub fn default_id(&self) -> &SessionId {
        &self.default_id
    }
    pub fn selector_revision(&self) -> Option<i64> {
        self.selector_revision
    }
    pub fn is_private(&self) -> bool {
        self.scope == AdmissionScope::Private
    }
    pub fn journal(&self) -> SessionJournal {
        SessionJournal::Root(self.session.id.clone())
    }
    pub fn root_history_key(&self) -> String {
        format!("session/{}", self.session.id)
    }

    pub(crate) fn check(&self, session: &Session) -> SessionResult<()> {
        if !self.is_live() {
            return Err(SessionError::Ended);
        }
        require_host(&session.host)?;
        let permitted = match (&session.access, self.scope) {
            (SessionAccess::Private { owner }, AdmissionScope::Private) => self
                .user
                .as_ref()
                .is_some_and(|user| owner == user.as_str()),
            (SessionAccess::GroupBound { connector, channel }, AdmissionScope::Group) => {
                connector == self.reply.connector() && channel == self.reply.channel()
            }
            _ => false,
        };
        if !permitted {
            return Err(SessionError::Denied);
        }
        Ok(())
    }
    pub(crate) fn require_private(&self) -> SessionResult<()> {
        if self.is_private() {
            Ok(())
        } else {
            Err(SessionError::GroupSelector)
        }
    }
}

pub(crate) fn cleanup_scope_for(route: &ReplyRoute, session_id: &SessionId) -> String {
    let value = to_vec(&(route.connector(), route.channel(), session_id.as_str()))
        .expect("a string tuple serializes");
    format!("cleanup/{:x}", Sha256::digest(value))
}

/// Only host policy emits this non-serializable cancellation token. A JSON
/// message or model dispatch cannot manufacture a trusted Session close event.
#[derive(Clone)]
pub(crate) struct SessionEnded {
    pub session_id: SessionId,
}

/// Inventory and binding are supplied by a trusted source adapter/operator, not
/// reconstructed from a negative chat ID or from text in a stored transcript.
#[derive(Clone, Debug)]
#[cfg(test)]
pub(crate) struct VerifiedLegacyImport {
    pub source: JournalKeyInventory,
    pub origin: SessionOrigin,
    pub session_id: SessionId,
    pub access: SessionAccess,
    pub children: Vec<VerifiedLegacyChild>,
}

#[derive(Clone, Debug)]
#[cfg(test)]
pub(crate) struct VerifiedLegacyChild {
    pub source: JournalKeyInventory,
    pub run_id: String,
}

pub(crate) fn require_host(host: &str) -> SessionResult<()> {
    if host == "host" {
        Ok(())
    } else {
        Err(SessionError::UnsupportedHost(host.into()))
    }
}

pub(crate) fn validate_identifier(value: &str) -> SessionResult<()> {
    if value.is_empty() || value.len() > 1024 || value.trim() != value || value.contains('\0') {
        Err(SessionError::InvalidInput("invalid identity or origin"))
    } else {
        Ok(())
    }
}
