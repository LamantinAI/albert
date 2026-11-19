use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::{IntoParams, ToSchema};

use super::{AttachmentView, ProgressSnapshot, SelectedSkillView, SkillSelection, TextSnapshot};

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
    #[serde(default)]
    pub attachments: Vec<String>,
    #[serde(default)]
    pub skills: Vec<SkillSelection>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MessageSource {
    Telegram,
}

#[derive(Clone, Serialize, ToSchema)]
pub struct MessageView {
    pub id: String,
    pub created_at: String,
    pub role: String,
    pub content: String,
    /// Recognized channel of the original message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<MessageSource>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[schema(required = false)]
    pub attachments: Vec<AttachmentView>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[schema(required = false)]
    pub skills: Vec<SelectedSkillView>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<Box<ProgressSnapshot>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<Box<TextSnapshot>>,
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
#[derive(Clone, Default, Deserialize, IntoParams, ToSchema)]
#[into_params(parameter_in = Query)]
pub struct PageQuery {
    pub after: Option<String>,
    pub limit: Option<usize>,
    pub order: Option<String>,
    pub through_id: Option<String>,
}

#[derive(Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct EventQuery {
    pub after: Option<String>,
    pub runtime_epoch: Option<String>,
}
