use std::{collections::HashSet, result::Result as ToolResult, sync::Arc, sync::Weak};

use octo_core::control::CancellationToken;
use octo_history::SessionKind;
use octo_mcp::{CallToolResult, JsonObject, McpError};
use rig::{
    completion::ToolDefinition, tool::ToolDyn, tool::ToolError, wasm_compat::WasmBoxedFuture,
};
use serde_json::{from_str, to_string, Value};
use sha2::{Digest, Sha256};
use tokio::select;

use super::{McpRegistry, McpRegistryError, Result, ServerPhase};
use crate::{accounts::AccountRole, sessions::actor::ActorToolPolicy, sessions::SessionContext};

#[derive(Clone)]
pub struct McpTool {
    registry: Weak<McpRegistry>,
    server: String,
    generation: u64,
    revision: String,
    remote: String,
    definition: ToolDefinition,
    actor: SessionContext,
    grant: CancellationToken,
}
impl McpRegistry {
    pub fn snapshot(self: &Arc<Self>, actor: Option<&SessionContext>) -> Result<Vec<McpTool>> {
        let Some(actor) = actor.filter(|actor| allowed(actor)) else {
            return Ok(vec![]);
        };
        let entries = self.entries.read().unwrap();
        let mut tools = Vec::new();
        let mut names = HashSet::new();
        for (id, entry) in entries
            .iter()
            .filter(|(_, entry)| entry.spec.enabled && entry.contract_valid)
        {
            let Some(client) = &entry.client else {
                continue;
            };
            let snapshot = client.snapshot();
            for tool in snapshot.tools.iter().filter(|tool| {
                entry
                    .spec
                    .allowed_tools
                    .iter()
                    .any(|name| name == tool.name.as_ref())
            }) {
                let name = tool_name(id, tool.name.as_ref());
                if !names.insert(name.clone()) {
                    return Err(McpRegistryError::Collision);
                }
                tools.push(McpTool {
                    registry: Arc::downgrade(self),
                    server: id.clone(),
                    generation: entry.generation,
                    revision: snapshot.revision.clone(),
                    remote: tool.name.to_string(),
                    definition: ToolDefinition {
                        name,
                        description: format!(
                            "{}\nMCP server: {}.",
                            tool.description.as_deref().unwrap_or("External MCP tool"),
                            entry.spec.name
                        ),
                        parameters: Value::Object((*tool.input_schema).clone()),
                    },
                    actor: actor.clone(),
                    grant: entry
                        .grants
                        .get(tool.name.as_ref())
                        .ok_or(McpRegistryError::Stale)?
                        .clone(),
                });
            }
        }
        Ok(tools)
    }
    async fn invoke(&self, tool: &McpTool, arguments: JsonObject) -> Result<CallToolResult> {
        if tool.grant.is_cancelled() {
            return Err(McpRegistryError::Stale);
        }
        if !allowed(&tool.actor) || !tool.actor.authorized().await {
            return Err(McpRegistryError::Access);
        }
        let (client, cancel) = {
            let entries = self.entries.read().unwrap();
            let entry = entries
                .get(&tool.server)
                .filter(|entry| {
                    entry.spec.enabled
                        && entry.contract_valid
                        && entry.generation == tool.generation
                        && entry.spec.allowed_tools.contains(&tool.remote)
                })
                .ok_or(McpRegistryError::Stale)?;
            let client = entry.client.clone().ok_or(McpRegistryError::Stale)?;
            if client.snapshot().revision != tool.revision {
                return Err(McpRegistryError::Stale);
            }
            let cancel = tool.grant.child_token();
            (client, cancel)
        };
        let result = select! {biased;_=tool.actor.access_cancelled()=>return Err(McpError::Indeterminate.into()),result=client.call(&tool.revision,&tool.remote,arguments,cancel)=>result};
        if let Err(error) = &result {
            if let Some(entry) = self
                .entries
                .write()
                .unwrap()
                .get_mut(&tool.server)
                .filter(|entry| entry.generation == tool.generation)
            {
                entry.error = Some(error.to_string());
                if *error == McpError::SchemaChanged {
                    entry.contract_valid = false;
                    entry.phase = ServerPhase::Error;
                }
            }
        }
        result.map_err(Into::into)
    }
}
impl ToolDyn for McpTool {
    fn name(&self) -> String {
        self.definition.name.clone()
    }
    fn definition(&self, _prompt: String) -> WasmBoxedFuture<'_, ToolDefinition> {
        Box::pin(async { self.definition.clone() })
    }
    fn call(&self, arguments: String) -> WasmBoxedFuture<'_, ToolResult<String, ToolError>> {
        Box::pin(async move {
            let parsed = from_str::<Value>(&arguments)
                .ok()
                .and_then(|value| value.as_object().cloned())
                .ok_or(McpRegistryError::Arguments);
            let result = async {
                let registry = self.registry.upgrade().ok_or(McpRegistryError::Stale)?;
                let result = registry.invoke(self, parsed?).await?;
                to_string(&result).map_err(|_| McpRegistryError::Arguments)
            }
            .await;
            result.map_err(|error| ToolError::ToolCallError(Box::new(error)))
        })
    }
}
pub fn tool_name(server: &str, remote: &str) -> String {
    fn slug(value: &str, max: usize) -> String {
        value
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .take(max)
            .collect()
    }
    let digest = format!(
        "{:x}",
        Sha256::digest(format!("{server}\0{remote}").as_bytes())
    );
    format!(
        "mcp_{}_{}_{}",
        slug(server, 14),
        slug(remote, 20),
        &digest[..20]
    )
}
fn allowed(actor: &SessionContext) -> bool {
    actor.is_live()
        && actor.is_private()
        && actor.session().kind == SessionKind::Permanent
        && actor.tool_policy() == ActorToolPolicy::ExistingHost
        && actor
            .account()
            .is_some_and(|account| account.role == AccountRole::Owner)
}
