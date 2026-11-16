//! Scope and namespace external memory verbs without duplicating their schemas.

use std::sync::Arc;

use rig::{
    completion::ToolDefinition, tool::ToolDyn, tool::ToolError, wasm_compat::WasmBoxedFuture,
};
use rmcp::{model::JsonObject, model::Tool};
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
            default_scope(&self.definition, &mut args);
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

fn default_scope(definition: &Tool, args: &mut JsonObject) {
    let name = definition.name.as_ref();
    if name == "search" {
        if args.get("initiatives").is_none_or(Value::is_null) {
            args.insert(
                "initiatives".into(),
                Value::Array(vec![Value::String(INITIATIVE.into())]),
            );
        }
        return;
    }
    if (name == "context" && args.get("view").and_then(Value::as_str) == Some("graph"))
        || (name == "export" && (args.contains_key("initiatives") || args.contains_key("all")))
    {
        return;
    }
    let schema = &definition.input_schema;
    let accepts = schema
        .get("properties")
        .and_then(Value::as_object)
        .is_some_and(|props| props.contains_key("initiative"))
        || schema
            .get("oneOf")
            .and_then(Value::as_array)
            .is_some_and(|variants| {
                variants.iter().any(|variant| {
                    variant.pointer("/properties/initiative").is_some()
                        && variant
                            .pointer("/properties/action/const")
                            .is_none_or(|action| {
                                args.get("action").is_none_or(|provided| provided == action)
                            })
                })
            });
    if accepts && args.get("initiative").is_none_or(Value::is_null) {
        args.insert("initiative".into(), Value::String(INITIATIVE.into()));
    }
}
