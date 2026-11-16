//! Dedicated memory configuration, independent of the general MCP registry.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::models::ConfigDocument;

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryBackend {
    Embedded,
    Mcp,
}
#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryTransport {
    Http,
    Stdio,
}
#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MemorySettings {
    pub backend: MemoryBackend,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport: Option<MemoryTransport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_env: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    /// Child environment variable to saved credential name, never its value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<BTreeMap<String, String>>,
}
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryWrite {
    pub revision: String,
    pub config: MemorySettings,
    /// Write-only bearer token. Omit to retain the saved value.
    #[schema(write_only)]
    pub token_value: Option<String>,
    /// Agent draft with its original memory section; selection is overlaid
    /// after both the global and document revisions have been checked.
    pub document: Option<MemoryDocument>,
}
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryDocument {
    pub content: String,
    pub revision: String,
}
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryInitialization {
    pub revision: String,
}
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryPhase {
    SelectionPending,
    Saved,
    Initializing,
    Active,
    RestartRequired,
    InitializationFailed,
}
#[derive(Serialize, ToSchema)]
pub struct MemoryView {
    pub config: MemorySettings,
    pub revision: String,
    pub phase: MemoryPhase,
    pub selection_required: bool,
    pub model_configured: bool,
    pub token_configured: bool,
    pub credential_names: Vec<String>,
    pub error: Option<String>,
    /// Confirmed merged document, returned only for a combined save.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document: Option<ConfigDocument>,
}
pub enum MemoryOperation {
    Get,
    Put(MemoryWrite),
    Initialize(MemoryInitialization),
}
