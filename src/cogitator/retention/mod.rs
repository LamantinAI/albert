use std::{io::Error as IoError, sync::Arc};

use octo_approval::{Authority, Ingress, ScopeBinding};
use octo_history::SessionKind;
use rig::{
    completion::ToolDefinition,
    tool::{ToolDyn, ToolError},
    wasm_compat::WasmBoxedFuture,
};

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
    let Some(epoch) = session.scope_epoch() else {
        return Ok(Some(ingress.clone()));
    };
    let binding = ScopeBinding::new(session.root_history_key(), epoch)?;
    authority.bind_scope(ingress, binding).map(Some)
}

/// Reviewed against pinned Kaeru 0.7.5 lookup/chains/manage implementations.
/// Mixed read/write verbs, Awake, unknown names and all capture/export verbs are
/// excluded. Updating a backend's tool surface requires an explicit review.
pub(super) fn memory_read(name: &str) -> bool {
    matches!(
        name,
        "kaeru_recall"
            | "kaeru_search"
            | "kaeru_at"
            | "kaeru_history"
            | "kaeru_neighbours"
            | "kaeru_drill"
            | "kaeru_trace"
            | "kaeru_tagged"
            | "kaeru_between"
            | "kaeru_surface"
            | "kaeru_ideas"
            | "kaeru_outcomes"
            | "kaeru_overview"
            | "kaeru_initiatives"
            | "kaeru_recent"
            | "kaeru_why"
            | "kaeru_path"
            | "kaeru_lint"
            | "kaeru_reflect"
            | "kaeru_config"
            | "kaeru_clouds"
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
            if !self.session.is_live() {
                return Err(ToolError::ToolCallError(Box::new(IoError::other(
                    "Session ended; tool was not executed.",
                ))));
            }
            self.inner.call(args).await
        })
    }
}
