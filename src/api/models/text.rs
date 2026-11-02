use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct TextDelta {
    pub attempt_id: String,
    pub generation: String,
    pub operation: String,
    pub offset: usize,
    pub text: String,
    pub message_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
pub struct TextSnapshot {
    pub attempt_id: String,
    pub generation: String,
    pub text: String,
    pub offset: usize,
    pub state: String,
    pub truncated: bool,
    pub message_ids: Vec<String>,
}
