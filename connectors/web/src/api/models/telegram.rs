use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TelegramCredentialWrite {
    pub bot_token: String,
}

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TelegramTokenRequest {}

#[derive(Serialize, ToSchema)]
pub struct TelegramLoginTokenView {
    pub id: String,
    pub token: String,
    pub expires_at: i64,
}

#[derive(Serialize, ToSchema)]
pub struct TelegramBindingView {
    pub actor_id: String,
    pub chat_id: String,
    pub created_at: i64,
}

#[derive(Serialize, ToSchema)]
pub struct TelegramConnectionView {
    pub connector_id: String,
    pub connected: bool,
    pub bot_username: Option<String>,
    pub requires_restart: bool,
    pub bindings: Vec<TelegramBindingView>,
}

#[derive(Serialize, ToSchema)]
pub struct TelegramConnectionList {
    pub items: Vec<TelegramConnectionView>,
}
