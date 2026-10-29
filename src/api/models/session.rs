use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

#[derive(Clone, Serialize, ToSchema)]
pub struct SessionView {
    pub id: String,
    pub title: String,
    pub host: String,
    pub kind: String,
    pub state: String,
    pub revision: i64,
    pub created_at: String,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct SessionList {
    pub items: Vec<SessionView>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}
#[derive(Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionCreate {
    pub title: Option<String>,
    #[serde(default)]
    pub temporary: bool,
}
#[derive(Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionUpdate {
    pub revision: i64,
    pub title: Option<String>,
    pub archived: Option<bool>,
    pub kind: Option<String>,
    pub state: Option<String>,
    pub promotion_key: Option<String>,
}
#[derive(Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MessageCreate {
    pub client_message_id: String,
    pub text: String,
    pub digest: Option<String>,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct MessageView {
    pub id: String,
    pub created_at: String,
    pub role: String,
    pub content: String,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct MessageList {
    pub items: Vec<MessageView>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
    pub through_id: Option<String>,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct MessageReceipt {
    pub client_message_id: String,
    pub attempt_id: Option<String>,
    pub message_ids: Vec<String>,
    pub duplicate: bool,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct AttemptView {
    pub id: String,
    pub state: String,
    pub session_id: String,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct SessionState {
    pub session: SessionView,
    pub active_attempt: Option<AttemptView>,
    pub runtime_epoch: String,
    pub status: Vec<String>,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct SessionEvent {
    pub id: String,
    pub session_id: String,
    pub runtime_epoch: String,
    pub kind: String,
    pub attempt_id: Option<String>,
    pub data: Value,
}
#[derive(Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CancelRequest {
    pub session_id: String,
}
#[derive(Clone, Default, Deserialize, ToSchema)]
pub struct PageQuery {
    pub after: Option<String>,
    pub limit: Option<usize>,
    pub order: Option<String>,
    pub through_id: Option<String>,
}
