use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::{IntoParams, ToSchema};

#[derive(Clone, Serialize, Deserialize, ToSchema)]
#[serde(tag = "transport", rename_all = "snake_case", deny_unknown_fields)]
pub enum McpConnection {
    Http {
        url: String,
        token_env: Option<String>,
    },
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        inherited_env: Option<Vec<String>>,
    },
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct McpServerConfig {
    pub id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub connection: McpConnection,
    pub allowed_tools: Vec<String>,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct McpServerView {
    pub config: McpServerConfig,
    pub phase: String,
    pub tool_count: usize,
    pub error: Option<String>,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct McpServerList {
    pub items: Vec<McpServerView>,
    pub revision: String,
}
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct McpServerWrite {
    pub config: McpServerConfig,
    pub revision: String,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct McpToolView {
    pub name: String,
    pub description: String,
    pub allowed: bool,
    pub schema: Option<Value>,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct McpToolList {
    pub items: Vec<McpToolView>,
    pub revision: String,
    pub offset: usize,
    pub next_offset: Option<usize>,
    pub total: usize,
    pub missing_allowed: Vec<String>,
}
#[derive(Default, Deserialize, IntoParams)]
#[into_params(parameter_in=Query)]
#[serde(deny_unknown_fields)]
pub struct McpToolQuery {
    pub offset: Option<usize>,
    pub limit: Option<usize>,
    pub include_schema: Option<bool>,
}
