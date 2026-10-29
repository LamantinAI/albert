use sqlx::FromRow;

/// Private persistence record. Password hashes are never response DTO fields.
#[derive(Clone, FromRow)]
pub(crate) struct LoginAccount {
    pub id: String,
    pub username: String,
    pub password_hash: String,
    pub role: String,
    pub canonical_user_id: Option<String>,
    pub must_change_password: bool,
    pub disabled: bool,
    pub token_version: i64,
    pub revision: i64,
}
#[derive(FromRow)]
pub(crate) struct BrowserCredential {
    pub id: String,
    pub account_id: String,
    pub token_version: i64,
    pub issued_at: i64,
    pub expires_at: i64,
    pub revoked_at: Option<i64>,
}
