//! Transport-neutral live status values. HTTP schemas are owned by the web adapter.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgressActivity {
    Scratchpad,
    Memory,
    Skill,
    Connector,
    Search,
    Browser,
    Files,
    Shell,
    Sql,
    Calendar,
    Mail,
    Image,
    AudioInput,
    AudioOutput,
    Subagent,
    Sessions,
    Configuration,
    Model,
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProgressItem {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub state: String,
    pub started_at: String,
    pub updated_at: String,
    pub elapsed_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity: Option<ProgressActivity>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProgressDelta {
    pub attempt_id: String,
    pub at: String,
    pub replace_kind: Option<String>,
    pub items: Vec<ProgressItem>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TextDelta {
    pub attempt_id: String,
    pub generation: String,
    pub operation: String,
    pub offset: usize,
    pub text: String,
    pub message_ids: Vec<String>,
}
