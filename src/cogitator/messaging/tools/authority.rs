use octo_core::EventKind;

use super::SessionMessagingTool;
use crate::messaging::{
    contract::{ConnectorGrant, DelegationCeiling, MessageIntent},
    MessagingError, MessagingResult,
};

pub(in crate::cogitator::messaging) fn ordinary_kind(kind: &str) -> bool {
    !["sql.", "octo.", "chat.", "albert."]
        .iter()
        .any(|prefix| kind.starts_with(prefix))
        && !kind.contains('*')
        && kind.len() <= 1024
        && !kind.is_empty()
}
impl SessionMessagingTool {
    pub(super) fn available_grants(&self) -> Vec<ConnectorGrant> {
        self.ctx
            .connectors()
            .iter()
            .filter(|connector| connector.capabilities.description.is_some())
            .filter_map(|connector| {
                let kinds: Vec<_> = connector
                    .capabilities
                    .event_kinds_accept
                    .iter()
                    .map(|kind| kind.as_str())
                    .filter(|kind| ordinary_kind(kind))
                    .take(32)
                    .map(str::to_owned)
                    .collect();
                (!kinds.is_empty()).then(|| ConnectorGrant {
                    connector: connector.id.to_string(),
                    kinds,
                })
            })
            .take(32)
            .collect()
    }
    pub(super) fn clamp(
        &self,
        intent: MessageIntent,
        ceiling: Option<DelegationCeiling>,
    ) -> MessagingResult<Option<DelegationCeiling>> {
        if intent != MessageIntent::WorkRequest {
            if ceiling.is_some() {
                return Err(MessagingError::Invalid("only work requests delegate tools"));
            }
            return Ok(None);
        }
        let mut ceiling = ceiling.ok_or(MessagingError::Invalid(
            "work request requires explicit connector/kind ceiling",
        ))?;
        if ceiling.grants.is_empty()
            || ceiling.grants.len() > 32
            || ceiling.max_tool_calls == 0
            || ceiling.max_output_bytes == 0
        {
            return Err(MessagingError::Invalid("invalid work ceiling"));
        }
        for grant in &ceiling.grants {
            let connector = self
                .ctx
                .connectors()
                .iter()
                .find(|connector| {
                    connector.id.as_str() == grant.connector
                        && connector.capabilities.description.is_some()
                })
                .ok_or(MessagingError::Denied)?;
            if grant.kinds.is_empty() || grant.kinds.len() > 32 {
                return Err(MessagingError::Denied);
            }
            for kind in &grant.kinds {
                if !ordinary_kind(kind)
                    || !connector
                        .capabilities
                        .event_kinds_accept
                        .iter()
                        .any(|pattern| EventKind::new(kind).matches(pattern.as_str()))
                {
                    return Err(MessagingError::Denied);
                }
            }
        }
        ceiling.max_tool_calls = ceiling
            .max_tool_calls
            .min(self.runtime.settings.max_tool_calls);
        ceiling.max_output_bytes = ceiling
            .max_output_bytes
            .min(self.runtime.settings.max_output_bytes);
        Ok(Some(ceiling))
    }
}
