use std::{io::Error as IoError, sync::Arc};

use octo_approval::{Authority, Ingress, ScopeBinding};
use octo_history::SessionKind;
use rig::{
    completion::ToolDefinition, tool::ToolDyn, tool::ToolError, wasm_compat::WasmBoxedFuture,
};
use tokio::select;

use crate::sessions::SessionContext;
mod end;

pub(super) fn temporary(session: Option<&SessionContext>) -> bool {
    session.is_some_and(|session| session.session().kind == SessionKind::Temporary)
}

/// Bind native database authority to the same immutable assembly lifetime for
/// every transport, before building either root or delegated child tools.
pub(super) fn scope_ingress(
    authority: &Authority,
    ingress: Option<Ingress>,
    session: Option<&SessionContext>,
) -> Result<Option<Ingress>, String> {
    let (Some(ingress), Some(session)) = (ingress.as_ref(), session) else {
        return Ok(ingress);
    };
    // Account-native ingress is already bound to its exact live account proof
    // at the source adapter. Retain it when constructing root and child tools.
    if session.account().is_some() {
        return Ok(Some(ingress.clone()));
    }
    let Some(epoch) = session.scope_epoch() else {
        return Ok(Some(ingress.clone()));
    };
    let binding = ScopeBinding::new(session.root_history_key(), epoch)?;
    authority.bind_scope(ingress, binding).map(Some)
}

/// Only canonical Kaeru 0.8 read-only operations are available in volatile
/// conversations. Context records reminder delivery metadata and is excluded.
pub(super) fn memory_read(name: &str) -> bool {
    matches!(
        name,
        "kaeru_search"
            | "kaeru_read"
            | "kaeru_reflect"
            | "kaeru_history"
            | "kaeru_initiatives"
            | "kaeru_clouds"
            | "kaeru_cloud_initiatives"
            | "kaeru_cloud_recall"
            | "kaeru_cloud_links"
            | "kaeru_sync_review"
    )
}

pub(super) fn permitted_memory(
    tools: Vec<Box<dyn ToolDyn>>,
    volatile: bool,
) -> Vec<Box<dyn ToolDyn>> {
    tools
        .into_iter()
        .filter(|tool| !volatile || memory_read(&tool.name()))
        .collect()
}

/// Host-only fencing surrounds every installed tool, including explicit file
/// and connector tools. It never broadens permissions or reconstructs a lease.
pub(super) fn fence_tools(
    tools: Vec<Box<dyn ToolDyn>>,
    session: Option<SessionContext>,
) -> Vec<Box<dyn ToolDyn>> {
    let Some(session) = session else {
        return tools;
    };
    tools
        .into_iter()
        .map(|inner| {
            Box::new(FencedTool {
                inner: Arc::from(inner),
                session: session.clone(),
            }) as Box<dyn ToolDyn>
        })
        .collect()
}

struct FencedTool {
    inner: Arc<dyn ToolDyn>,
    session: SessionContext,
}

impl ToolDyn for FencedTool {
    fn name(&self) -> String {
        self.inner.name()
    }
    fn definition(&self, prompt: String) -> WasmBoxedFuture<'_, ToolDefinition> {
        self.inner.definition(prompt)
    }
    fn call(&self, args: String) -> WasmBoxedFuture<'_, Result<String, ToolError>> {
        Box::pin(async move {
            if !self.session.authorized().await {
                return Err(ToolError::ToolCallError(Box::new(IoError::other(
                    "Session ended or account access changed; tool was not executed.",
                ))));
            }
            select! {
                biased;
                _ = self.session.access_cancelled() => Err(ToolError::ToolCallError(Box::new(IoError::other("Account access changed; pending tool stopped.")))),
                result = self.inner.call(args) => result,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::memory_read;
    use kaeru_core::api::tool_definitions;

    #[test]
    fn temporary_memory_tools_never_include_canonical_mutations_or_context_delivery_bookkeeping() {
        for definition in tool_definitions() {
            let name = format!("kaeru_{}", definition.name);
            if memory_read(&name) {
                assert!(definition.annotations.read_only_hint, "{name}");
            }
        }
        assert!(!memory_read("kaeru_context"));
        assert!(!memory_read("kaeru_note"));
        assert!(!memory_read("kaeru_unknown_future_mutation"));
    }
}
