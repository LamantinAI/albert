use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

#[derive(Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    pub id: String,
    pub model: String,
    pub provider: String,
    pub base_url: Option<String>,
    pub api_key_env: Option<String>,
    pub context_window: Option<usize>,
    pub vision: bool,
    pub tools: bool,
    pub reasoning: Option<Value>,
    pub request_timeout_ms: Option<u64>,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct ModelView {
    pub config: ModelConfig,
    pub credential_configured: bool,
    pub priority: usize,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct ModelPoolView {
    pub revision: String,
    pub preferred: Option<String>,
    pub models: Vec<ModelView>,
    pub roles: Vec<ModelRole>,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct ModelRole {
    pub id: String,
    pub description: String,
    pub models: Vec<String>,
    pub reasoning_effort: Option<String>,
}
#[derive(Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelWrite {
    pub config: ModelConfig,
    pub api_key: Option<String>,
    pub revision: Option<String>,
}
#[derive(Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelOrder {
    pub ids: Vec<String>,
    pub revision: String,
}
#[derive(Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelPreference {
    pub model_id: String,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct SkillView {
    pub id: String,
    pub name: String,
    pub description: String,
    pub command: Option<String>,
    pub owner_only: bool,
    pub enabled: bool,
    pub content: Option<String>,
    pub revision: String,
}
#[derive(Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SkillUpdate {
    pub revision: String,
    pub enabled: bool,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct SkillList {
    pub items: Vec<SkillView>,
}
#[derive(Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SkillWrite {
    pub content: String,
    pub revision: Option<String>,
}
#[derive(Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SkillImport {
    pub filename: String,
    pub data_base64: String,
    #[serde(default)]
    pub replace: bool,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct ConfigDocument {
    pub path: String,
    pub content: String,
    pub revision: String,
    pub redacted: bool,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct ConfigList {
    pub items: Vec<String>,
    pub revision: String,
}
#[derive(Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigWrite {
    pub path: String,
    pub content: String,
    pub revision: Option<String>,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct ValidationView {
    pub valid: bool,
    pub issues: Vec<String>,
    pub requires_restart: bool,
}
#[derive(Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyRequest {
    pub target: String,
    pub revision: Option<String>,
    pub client_request_id: String,
    pub document_id: Option<String>,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct ApplyView {
    pub id: String,
    pub revision: String,
    pub state: String,
    pub runtime_epoch: String,
    pub message: String,
}
#[derive(Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SecretWrite {
    pub value: String,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct SecretList {
    pub items: Vec<String>,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct ApprovalView {
    pub id: String,
    pub connector: String,
    pub state: String,
    pub summary: Value,
    pub expires_at: Option<i64>,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct ApprovalList {
    pub items: Vec<ApprovalView>,
}
#[derive(Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ApprovalDecision {
    pub approve: bool,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct RuntimeStatus {
    pub phase: String,
    pub model_ready: bool,
    pub runtime_epoch: String,
    pub error: Option<String>,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct EmptyObject {}
