use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct OpenAiLoginStart {
    #[serde(default)]
    pub replace_existing: bool,
}
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct OpenAiLoginSubmission {
    pub callback: String,
}
#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum OpenAiLoginPhase {
    AwaitingCallback,
    Exchanging,
    Connected,
    Failed,
    Cancelled,
    Expired,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct OpenAiLoginView {
    pub id: String,
    pub phase: OpenAiLoginPhase,
    pub authorize_url: Option<String>,
    pub redirect_uri: String,
    pub expires_at: String,
    pub credentials_saved: bool,
    pub message: Option<String>,
}
#[derive(Clone, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionPhase {
    Disconnected,
    Connected,
    Applying,
    Ready,
    ApplyFailed,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct SubscriptionConnectionView {
    pub connected: bool,
    pub generation: Option<String>,
    pub phase: SubscriptionPhase,
    pub selected_model_id: Option<String>,
    pub selected_slug: Option<String>,
    pub preferred_model_id: Option<String>,
    pub message: Option<String>,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct SubscriptionCatalogModel {
    pub slug: String,
    pub selectable: bool,
    pub unavailable_reason: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub input_modalities: Vec<String>,
    pub context_window: Option<usize>,
    pub reasoning_efforts: Vec<String>,
    pub default_reasoning_effort: Option<String>,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct SubscriptionCatalogView {
    pub generation: String,
    pub revision: String,
    pub models: Vec<SubscriptionCatalogModel>,
}
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SubscriptionSelection {
    pub generation: String,
    pub catalog_revision: String,
    pub slug: String,
    #[serde(default)]
    pub prefer: bool,
}
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SubscriptionApplyRequest {
    pub generation: String,
}
