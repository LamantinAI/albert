use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AccountRole {
    Owner,
    User,
}

#[derive(Clone, Serialize, ToSchema)]
pub struct AccountView {
    pub id: String,
    pub username: String,
    pub role: AccountRole,
    pub canonical_user_id: Option<String>,
    pub must_change_password: bool,
    pub disabled: bool,
    pub revision: i64,
}
#[derive(Clone, Default, Serialize, ToSchema)]
pub struct Capabilities {
    pub owner: bool,
    pub chat: bool,
    pub manage_users: bool,
    pub manage_models: bool,
    pub manage_skills: bool,
    pub manage_config: bool,
}
#[derive(Clone, Copy, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SetupPhase {
    Credentials,
    Profile,
    Model,
    Ready,
    RuntimeError,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct SetupView {
    pub phase: SetupPhase,
    pub model_ready: bool,
    pub identity_required: bool,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct ClientContext {
    pub account: Option<AccountView>,
    pub caps: Capabilities,
    pub setup: SetupView,
    pub runtime_epoch: String,
    pub csrf: Option<String>,
    pub current_credential_id: Option<String>,
}
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CredentialUpdate {
    pub current_password: String,
    pub new_password: String,
    pub new_username: Option<String>,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct CredentialView {
    pub id: String,
    pub expires_at: i64,
}
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UserCreate {
    pub username: String,
    pub password: String,
    pub role: AccountRole,
}
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UserUpdate {
    pub revision: i64,
    pub role: Option<AccountRole>,
    pub disabled: Option<bool>,
}
#[derive(Serialize, ToSchema)]
pub struct UserList {
    pub items: Vec<AccountView>,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct ProfileView {
    pub canonical_user_id: Option<String>,
    pub display_name: Option<String>,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct IdentityView {
    pub connector: String,
    pub external_actor_id: String,
}
#[derive(Clone, Serialize, ToSchema)]
pub struct IdentityList {
    pub items: Vec<IdentityView>,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct ApiErrorBody {
    pub error: ApiErrorDetail,
}
#[derive(Clone, Serialize, Deserialize, ToSchema)]
pub struct ApiErrorDetail {
    pub code: String,
    pub message: String,
}
