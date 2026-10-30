//! Admission limits and pure validation. Runtime capability checks are supplied
//! by the host through RouteGuard; no model proposal can implement this guard.
use octo_history::{Session, SessionAccess, SessionKind, SessionState};
use serde::{Deserialize, Serialize};
use serde_json::to_vec;

use super::{
    contract::{
        DelegationCeiling, DestinationSpec, DisplayRoute, MessageDraft, MessageIntent,
        TrustedSource, WorkRef,
    },
    MessagingError, MessagingResult,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MessagingLimits {
    pub max_payload_bytes: usize,
    pub max_metadata_bytes: usize,
    pub max_context_items: usize,
    pub max_messages: i64,
    pub max_inbox_pending: i64,
    pub max_outbox_pending: i64,
    pub max_attempts: i64,
    pub max_work_policies: i64,
    pub max_hops: u16,
}
impl Default for MessagingLimits {
    fn default() -> Self {
        Self {
            max_payload_bytes: 64 * 1024,
            max_metadata_bytes: 16 * 1024,
            max_context_items: 8,
            max_messages: 100_000,
            max_inbox_pending: 64,
            max_outbox_pending: 256,
            max_attempts: 3,
            max_work_policies: 128,
            max_hops: 8,
        }
    }
}
impl MessagingLimits {
    pub fn validate(&self) -> MessagingResult<()> {
        if self.max_payload_bytes == 0
            || self.max_payload_bytes > 1024 * 1024
            || self.max_metadata_bytes == 0
            || self.max_metadata_bytes > 64 * 1024
            || self.max_context_items > 64
            || !(1..=1_000_000).contains(&self.max_messages)
            || !(1..=10_000).contains(&self.max_inbox_pending)
            || !(1..=100_000).contains(&self.max_outbox_pending)
            || !(1..=1024).contains(&self.max_work_policies)
            || !(1..=16).contains(&self.max_attempts)
            || !(1..=32).contains(&self.max_hops)
        {
            return Err(MessagingError::Invalid("invalid messaging limits"));
        }
        Ok(())
    }
}

/// Synchronous, fresh host preflight while holding the lifecycle read lease.
/// Check conversational/session-admission capability, current actor permission,
/// route revocation and transport policy. Return Denied for revocation and
/// Unavailable for unsupported/unreachable routes. It must not perform I/O or
/// hold the transaction over model/tool/network waits. SQL rechecks ownership,
/// selector, archive and temporary/ended markers separately.
pub trait RouteGuard: Send + Sync {
    fn check(&self, actor: &str, route: &DisplayRoute) -> MessagingResult<()>;
}

pub(super) fn identifier(value: &str) -> MessagingResult<()> {
    if value.is_empty() || value.len() > 1024 || value.trim() != value || value.contains('\0') {
        return Err(MessagingError::Invalid("invalid identifier"));
    }
    Ok(())
}
pub(super) fn route(route: &DisplayRoute) -> MessagingResult<()> {
    identifier(&route.connector)?;
    identifier(&route.channel)
}
pub(super) fn destination(destination: &DestinationSpec) -> MessagingResult<()> {
    match destination {
        DestinationSpec::FixedSession {
            display: Some(display),
            ..
        } => route(display),
        DestinationSpec::ChannelActive { route: display } => route(display),
        _ => Ok(()),
    }
}
pub(super) fn private_session(session: &Session, actor: &str) -> MessagingResult<()> {
    if session.kind == SessionKind::Temporary {
        return Err(MessagingError::SaveRequired);
    }
    if session.state != SessionState::Active || session.host != "host" {
        return Err(MessagingError::Unavailable);
    }
    match &session.access {
        SessionAccess::Private { owner } if owner == actor => Ok(()),
        _ => Err(MessagingError::Denied),
    }
}
pub(super) fn ceiling(ceiling: &DelegationCeiling) -> MessagingResult<()> {
    if ceiling.grants.is_empty()
        || ceiling.grants.len() > 32
        || ceiling.max_tool_calls == 0
        || ceiling.max_tool_calls > 1024
        || ceiling.max_output_bytes == 0
        || ceiling.max_output_bytes > 1024 * 1024
    {
        return Err(MessagingError::Invalid("invalid delegation ceiling"));
    }
    for grant in &ceiling.grants {
        identifier(&grant.connector)?;
        if grant.kinds.is_empty() || grant.kinds.len() > 32 {
            return Err(MessagingError::Invalid("invalid connector kind ceiling"));
        }
        for kind in &grant.kinds {
            identifier(kind)?;
        }
    }
    Ok(())
}
pub(super) fn draft(
    source: &TrustedSource,
    draft: &MessageDraft,
    limits: &MessagingLimits,
    now: i64,
) -> MessagingResult<()> {
    let actor = source.context.private_user_id()?;
    if !source.context.is_live() {
        return Err(MessagingError::Unavailable);
    }
    private_session(source.context.session(), actor.as_str())?;
    match &source.work {
        WorkRef::Turn(id) | WorkRef::Child(id) => identifier(id)?,
    }
    if let Some(connector) = &source.execution.connector {
        identifier(connector)?;
    }
    if draft.id.as_str().len() != 32
        || !draft
            .id
            .as_str()
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(MessagingError::Invalid("invalid message ID"));
    }
    destination(&draft.destination)?;
    if draft.content.is_empty()
        || to_vec(&(&draft.content, &draft.context))?.len() > limits.max_payload_bytes
        || draft.context.len() > limits.max_context_items
    {
        return Err(MessagingError::Limit);
    }
    if draft.deadline_unix_ms <= now
        || draft.hops_remaining == 0
        || draft.hops_remaining > limits.max_hops
    {
        return Err(MessagingError::Invalid("expired deadline or hop budget"));
    }
    if draft.intent == MessageIntent::WorkRequest && draft.ceiling.is_none() {
        return Err(MessagingError::Invalid(
            "work request requires a delegation ceiling",
        ));
    }
    if let Some(value) = &draft.ceiling {
        ceiling(value)?;
    }
    for context in &draft.context {
        if context.reference.record_id <= 0 || context.through_id < context.reference.record_id {
            return Err(MessagingError::Invalid("invalid context watermark"));
        }
    }
    Ok(())
}
