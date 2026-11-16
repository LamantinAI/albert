use albert_core::credentials::CredentialView;
use serde::{Deserialize, Serialize};
use serde_json::to_value;
use utoipa::{IntoParams, ToSchema};

#[derive(Clone, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CredentialCreate {
    pub client_request_id: String,
    pub session_id: Option<String>,
    #[serde(default)]
    pub name: String,
}
/// This write-only input is deliberately not Debug/Serialize and never enters events.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CredentialSubmission {
    pub name: String,
    pub value: String,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct CredentialRequestView {
    pub id: String,
    pub session_id: Option<String>,
    pub suggested_name: String,
    pub name: Option<String>,
    pub source: String,
    pub state: String,
    pub notification: String,
    pub expires_at: i64,
    pub replaced: bool,
    #[serde(skip_serializing_if = "is_false")]
    #[schema(required = false)]
    pub replayed: bool,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct CredentialRequestList {
    pub items: Vec<CredentialRequestView>,
    pub existing_names: Vec<String>,
}
impl From<CredentialView> for CredentialRequestView {
    fn from(view: CredentialView) -> Self {
        Self {
            id: view.id,
            session_id: view.session_id,
            suggested_name: view.suggested_name,
            name: view.name,
            source: to_value(view.source)
                .expect("enum")
                .as_str()
                .expect("string")
                .into(),
            state: to_value(view.state)
                .expect("enum")
                .as_str()
                .expect("string")
                .into(),
            notification: to_value(view.notification)
                .expect("enum")
                .as_str()
                .expect("string")
                .into(),
            expires_at: view.expires_at,
            replaced: view.replaced,
            replayed: view.replayed,
        }
    }
}

fn is_false(value: &bool) -> bool {
    !value
}

#[derive(Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct CredentialQuery {
    pub session_id: Option<String>,
}
