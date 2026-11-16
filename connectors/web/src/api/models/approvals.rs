use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::{IntoParams, ToSchema};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalKind {
    Confirmation,
    Native,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalState {
    Pending,
    Deciding,
    Confirmed,
    Denied,
    Committed,
    Rejected,
    Cancelled,
    Expired,
    RolledBack,
    Unknown,
    Superseded,
}
impl ApprovalState {
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Confirmed
                | Self::Denied
                | Self::Committed
                | Self::Rejected
                | Self::Cancelled
                | Self::Expired
                | Self::RolledBack
                | Self::Superseded
        )
    }
}
#[derive(Clone, Serialize, ToSchema)]
pub struct ApprovalView {
    pub id: String,
    pub connector: String,
    pub kind: ApprovalKind,
    pub session_id: Option<String>,
    pub title: String,
    pub action: String,
    pub effects: Vec<String>,
    pub details: Option<Value>,
    pub state: ApprovalState,
    pub summary: Value,
    pub expires_at: Option<i64>,
    pub can_decide: bool,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct ApprovalList {
    pub items: Vec<ApprovalView>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}
#[derive(Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ApprovalDecision {
    pub approve: bool,
}
#[derive(Default, Deserialize, IntoParams)]
#[into_params(parameter_in=Query)]
#[serde(deny_unknown_fields)]
pub struct ApprovalListQuery {
    pub session_id: Option<String>,
}
#[derive(Default, Deserialize, IntoParams)]
#[into_params(parameter_in=Query)]
#[serde(deny_unknown_fields)]
pub struct ApprovalQuery {
    #[serde(default)]
    pub include_details: bool,
}
