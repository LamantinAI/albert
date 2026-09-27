//! Scope and namespace external memory verbs without duplicating their schemas.

use std::sync::Arc;

use rig::{
    completion::ToolDefinition,
    tool::{ToolDyn, ToolError},
    wasm_compat::WasmBoxedFuture,
};
use rmcp::model::{JsonObject, Tool};
use serde_json::{from_str, to_string, Value};

use super::{mcp::McpMemory, MemoryError, INITIATIVE};

pub struct MemoryTool {
    definition: Tool,
    memory: Arc<McpMemory>,
}

impl MemoryTool {
    pub fn new(definition: Tool, memory: Arc<McpMemory>) -> Self {
        Self { definition, memory }
    }
}

impl ToolDyn for MemoryTool {
    fn name(&self) -> String {
        format!("kaeru_{}", self.definition.name)
    }

    fn definition(&self, _prompt: String) -> WasmBoxedFuture<'_, ToolDefinition> {
        Box::pin(async move {
            ToolDefinition {
                name: self.name(),
                description: format!("{}\nDefault initiative: albert. Pass initiative explicitly to work in another project.",
                    self.definition.description.as_deref().unwrap_or("")),
                parameters: self.definition.schema_as_json_value(),
            }
        })
    }

    fn call(&self, args: String) -> WasmBoxedFuture<'_, Result<String, ToolError>> {
        Box::pin(async move {
            let mut args: JsonObject = from_str(&args).map_err(ToolError::JsonError)?;
            if self
                .definition
                .input_schema
                .get("properties")
                .and_then(Value::as_object)
                .is_some_and(|props| props.contains_key("initiative"))
                && args.get("initiative").is_none_or(|v| v.is_null())
            {
                args.insert("initiative".into(), Value::String(INITIATIVE.into()));
            }
            let result = self
                .memory
                .call(&self.definition.name, args)
                .await
                .map_err(|e: MemoryError| ToolError::ToolCallError(Box::new(e)))?;
            // Preserve structured results, text, resources and error semantics.
            // Never drop structuredContent in favour of a possibly empty text list.
            to_string(&result).map_err(ToolError::JsonError)
        })
    }
}
