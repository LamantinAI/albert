use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Debug, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SkillSelection {
    pub id: String,
    pub revision: String,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct ComposerSkill {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(required = false)]
    pub command: Option<String>,
    pub id: String,
    pub name: String,
    pub description: String,
    pub revision: String,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct SelectedSkillView {
    pub id: String,
    pub name: String,
    pub revision: String,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct ComposerView {
    pub speech_available: bool,
    pub can_select_skills: bool,
    pub can_compact: bool,
    pub can_restart: bool,
    pub can_reload_models: bool,
    pub max_file_bytes: usize,
    pub max_attachments: usize,
    pub max_selected_skills: usize,
    pub recording_max_seconds: u64,
    pub skills: Vec<ComposerSkill>,
}
#[derive(Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CompactCreate {
    pub client_request_id: String,
    #[serde(default)]
    pub instructions: String,
}
