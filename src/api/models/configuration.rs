use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

#[derive(Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigDraftInput {
    pub content: String,
    pub revision: String,
    #[serde(default)]
    pub changes: Vec<ConfigFieldChange>,
}

#[derive(Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigFieldChange {
    pub path: String,
    /// Null removes the explicit setting and restores its default.
    pub value: Value,
}

#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConfigFieldKind {
    Boolean,
    Integer,
    Text,
}

#[derive(Clone, Serialize, ToSchema)]
pub struct ConfigField {
    pub path: String,
    pub group: String,
    pub label: String,
    pub description: String,
    pub kind: ConfigFieldKind,
    pub value: Option<Value>,
    pub default_value: Value,
    pub minimum: Option<i64>,
    pub maximum: Option<i64>,
}

#[derive(Clone, Serialize, ToSchema)]
pub struct ConfigDraftView {
    pub content: String,
    pub revision: String,
    pub fields: Vec<ConfigField>,
}
