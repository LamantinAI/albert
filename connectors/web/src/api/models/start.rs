use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionStart {
    pub client_message_id: String,
    pub text: String,
    #[serde(default)]
    pub attachments: Vec<String>,
    #[serde(default)]
    pub skills: Vec<super::SkillSelection>,
    #[serde(default)]
    pub temporary: bool,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct SessionStartView {
    pub id: String,
    pub session_id: String,
    pub state: String,
    pub published: bool,
    pub temporary: bool,
}
