use albert_core::status::domain::TextDelta as DomainTextDelta;
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

impl From<DomainTextDelta> for TextDelta {
    fn from(value: DomainTextDelta) -> Self {
        Self {
            attempt_id: value.attempt_id,
            generation: value.generation,
            operation: value.operation,
            offset: value.offset,
            text: value.text,
            message_ids: value.message_ids,
        }
    }
}
