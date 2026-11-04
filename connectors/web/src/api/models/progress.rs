use albert_core::{
    status::domain::ProgressDelta as DomainProgressDelta,
    status::domain::ProgressItem as DomainProgressItem,
};
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

impl From<DomainProgressItem> for ProgressItem {
    fn from(value: DomainProgressItem) -> Self {
        Self {
            id: value.id,
            kind: value.kind,
            label: value.label,
            state: value.state,
            started_at: value.started_at,
            updated_at: value.updated_at,
            elapsed_ms: value.elapsed_ms,
        }
    }
}

impl From<DomainProgressDelta> for ProgressDelta {
    fn from(value: DomainProgressDelta) -> Self {
        Self {
            attempt_id: value.attempt_id,
            at: value.at,
            replace_kind: value.replace_kind,
            items: value.items.into_iter().map(Into::into).collect(),
        }
    }
}
