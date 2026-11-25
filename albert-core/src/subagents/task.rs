//! A bounded delegation request and its explicit model/tool assignments.
use crate::scratchpad::StageLink;
use serde::{Deserialize, Serialize};
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub task: String,
    #[serde(default)]
    pub context: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachment_ids: Vec<String>,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
    #[serde(default)]
    pub stage: Option<StageLink>,
    #[serde(default)]
    pub connectors: Vec<String>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub sql_stores: Vec<String>,
    #[serde(default)]
    pub sql_write: bool,
    #[serde(default)]
    pub sql_schema: bool,
    #[serde(default)]
    pub max_tool_turns: Option<usize>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}
