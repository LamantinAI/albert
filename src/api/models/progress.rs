use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct ProgressItem {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub state: String,
    pub started_at: String,
    pub updated_at: String,
    pub elapsed_ms: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct ProgressDelta {
    pub attempt_id: String,
    pub at: String,
    pub replace_kind: Option<String>,
    pub items: Vec<ProgressItem>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct ProgressSnapshot {
    pub attempt_id: String,
    pub started_at: String,
    pub updated_at: String,
    pub items: Vec<ProgressItem>,
    pub truncated: bool,
}
