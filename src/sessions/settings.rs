use octo_history::MAX_PAGE_SIZE;
use serde::{Deserialize, Serialize};

use super::{error::SessionResult, ExternalIdentity, UserId};
use crate::sessions::error::SessionError;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct SessionSettings {
    pub limits: SessionLimits,
    /// Trusted host configuration, never inferred from an ACL role.
    pub identity_links: Vec<IdentityLink>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct SessionLimits {
    pub temporary_max_sessions: usize,
    pub temporary_max_turns: usize,
    pub temporary_max_bytes: usize,
    pub max_page_size: u32,
    pub telegram_tracking_max_scopes: usize,
    pub telegram_tracking_max_ids: usize,
    pub telegram_cleanup_batches: usize,
    pub telegram_cleanup_timeout_ms: u64,
}

impl Default for SessionLimits {
    fn default() -> Self {
        Self {
            temporary_max_sessions: 32,
            temporary_max_turns: 10_000,
            temporary_max_bytes: 32 * 1024 * 1024,
            max_page_size: 100,
            telegram_tracking_max_scopes: 256,
            telegram_tracking_max_ids: 10_000,
            telegram_cleanup_batches: 4,
            telegram_cleanup_timeout_ms: 3_000,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct IdentityLink {
    pub connector: String,
    pub actor: String,
    pub user_id: String,
}

impl SessionSettings {
    pub(crate) fn validate(&self) -> SessionResult<()> {
        let limits = &self.limits;
        if limits.temporary_max_sessions == 0
            || limits.temporary_max_turns == 0
            || limits.temporary_max_bytes == 0
            || limits.max_page_size == 0
            || limits.max_page_size as usize > MAX_PAGE_SIZE
            || limits.telegram_tracking_max_scopes == 0
            || limits.telegram_tracking_max_ids == 0
            || limits.telegram_cleanup_batches == 0
            || limits.telegram_cleanup_timeout_ms == 0
        {
            return Err(SessionError::InvalidInput("invalid session limits"));
        }
        for (index, link) in self.identity_links.iter().enumerate() {
            ExternalIdentity::new(&link.connector, &link.actor)?;
            UserId::parse(&link.user_id)?;
            if self.identity_links[..index].iter().any(|previous| {
                previous.connector == link.connector && previous.actor == link.actor
            }) {
                return Err(SessionError::InvalidInput("duplicate identity link"));
            }
        }
        Ok(())
    }
}
