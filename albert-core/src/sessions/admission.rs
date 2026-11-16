//! Host-issued admission policy before session selection.
use super::{
    actor::ActorToolPolicy, error::SessionError, error::SessionResult, require_host,
    ExternalIdentity, ReplyRoute,
};
use crate::accounts::{AccountGrant, AccountRole};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdmissionScope {
    Private,
    Group,
}

#[derive(Clone, Debug)]
pub struct Admission {
    pub(crate) account: Option<AccountGrant>,
    pub(crate) identity: Option<ExternalIdentity>,
    pub(crate) route: ReplyRoute,
    pub(crate) scope: AdmissionScope,
    pub(crate) host: String,
}

impl Admission {
    pub fn route(&self) -> &ReplyRoute {
        &self.route
    }
    pub fn scope(&self) -> AdmissionScope {
        self.scope
    }
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
            account: None,
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
            account: None,
            identity,
            route,
            scope: AdmissionScope::Group,
            host: host.into(),
        })
    }
    pub fn with_account(mut self, account: AccountGrant) -> SessionResult<Self> {
        if let Some(binding) = &account.binding {
            if self.identity.as_ref().is_none_or(|identity| {
                identity.connector() != binding.connector || identity.actor() != binding.actor_id
            }) {
                return Err(SessionError::Denied);
            }
        }
        self.account = Some(account);
        Ok(self)
    }
    pub(crate) fn actor_policy(&self) -> Option<ActorToolPolicy> {
        self.account.as_ref().map(|account| {
            if account.role == AccountRole::Owner {
                ActorToolPolicy::ExistingHost
            } else {
                ActorToolPolicy::ChatOnly
            }
        })
    }
}
