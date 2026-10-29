use super::{
    errors::{ApiError, ApiResult},
    models::{AccountRole, AccountView},
};

/// Constructed only after JWT verification and current database account checks.
/// Not deserializable and never reconstructed from request role/user fields.
#[derive(Clone)]
pub struct Principal {
    account: AccountView,
    credential_id: String,
    csrf: String,
}
impl Principal {
    pub(crate) fn authenticated(account: AccountView, credential_id: String, csrf: String) -> Self {
        Self {
            account,
            credential_id,
            csrf,
        }
    }
    pub fn account(&self) -> &AccountView {
        &self.account
    }
    pub fn account_id(&self) -> &str {
        &self.account.id
    }
    pub fn role(&self) -> AccountRole {
        self.account.role
    }
    pub fn canonical_user_id(&self) -> Option<&str> {
        self.account.canonical_user_id.as_deref()
    }
    pub fn credential_id(&self) -> &str {
        &self.credential_id
    }
    pub fn csrf(&self) -> &str {
        &self.csrf
    }
    pub fn must_rotate(&self) -> bool {
        self.account.must_change_password
    }
    pub fn owner_permit(&self) -> ApiResult<OwnerPermit> {
        if self.account.role != AccountRole::Owner || self.account.disabled || self.must_rotate() {
            return Err(ApiError::forbidden());
        }
        Ok(OwnerPermit(self.clone()))
    }
}
#[derive(Clone)]
pub struct OwnerPermit(Principal);
impl OwnerPermit {
    pub fn principal(&self) -> &Principal {
        &self.0
    }
}
