use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use super::ProgressItem;

#[derive(Clone, Default, Deserialize, IntoParams, ToSchema)]
#[into_params(parameter_in = Query)]
pub struct OperatorQuery {
    /// Comma-separated IDs from the caller's private conversation catalog (up to 100).
    pub session_ids: String,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct OperatorCard {
    pub session_id: String,
    pub running: bool,
    pub outcome: Option<String>,
    pub active_children: usize,
    pub steps: Vec<ProgressItem>,
    /// available, unavailable, or restricted; absence is never a zero count.
    pub attention_state: String,
    pub approval_pending: Option<bool>,
    pub credential_pending: Option<bool>,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct OperatorSnapshot {
    pub catalog_changed: bool,
    pub runtime_epoch: String,
    pub items: Vec<OperatorCard>,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct ModelUsageView {
    pub model_id: String,
    pub model: String,
    pub provider: String,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cached_input_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub reported_calls: i64,
    pub unreported_calls: i64,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct ModelUsageSummary {
    pub tracking_started_at: String,
    pub period_started_at: String,
    pub models: Vec<ModelUsageView>,
    pub pending_records: usize,
    pub lost_records: u64,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct SubscriptionWindow {
    pub bucket: String,
    pub duration_seconds: u64,
    pub remaining_percent: f64,
    pub resets_at: i64,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct SubscriptionLimitsView {
    pub state: String,
    pub observed_at: Option<String>,
    pub windows: Vec<SubscriptionWindow>,
}
