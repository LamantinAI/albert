use super::{random_id, role_text, validate_username, view, AuthError, AuthService};
use crate::{
    api::{
        models::{AccountView, UserCreate, UserUpdate},
        principal::OwnerPermit,
    },
    db::repo::{
        account::{bind_profile_in, by_id, insert_with_profile, list, owners_in, update_in},
        identity::bind as bind_identity,
    },
};

impl AuthService {
    async fn check_owner(&self, permit: &OwnerPermit) -> Result<(), AuthError> {
        let account = by_id(&self.pool, permit.principal().account_id())
            .await?
            .ok_or(AuthError::Unauthorized)?;
        if account.disabled
            || account.must_change_password
            || account.role != "owner"
            || account.revision != permit.principal().account().revision
        {
            return Err(AuthError::Forbidden);
        }
        Ok(())
    }
    pub async fn list_users(&self, permit: &OwnerPermit) -> Result<Vec<AccountView>, AuthError> {
        self.check_owner(permit).await?;
        list(&self.pool).await?.into_iter().map(view).collect()
    }
    pub async fn create_user(
        &self,
        permit: &OwnerPermit,
        input: UserCreate,
    ) -> Result<AccountView, AuthError> {
        self.check_owner(permit).await?;
        validate_username(&input.username)?;
        if !(12..=1024).contains(&input.password.len()) {
            return Err(AuthError::InvalidInput(
                "Password must contain 12–1024 bytes.",
            ));
        }
        let hash = self.password_hash(input.password).await?;
        let id = random_id();
        let user = format!("user/web/{}", random_id());
        insert_with_profile(
            &self.pool,
            &id,
            &input.username,
            &hash,
            role_text(input.role),
            &user,
        )
        .await?;
        view(
            by_id(&self.pool, &id)
                .await?
                .ok_or(AuthError::Configuration)?,
        )
    }
    pub async fn update_user(
        &self,
        permit: &OwnerPermit,
        id: &str,
        input: UserUpdate,
    ) -> Result<AccountView, AuthError> {
        self.check_owner(permit).await?;
        let account = by_id(&self.pool, id)
            .await?
            .ok_or(AuthError::InvalidInput("Account not found."))?;
        if account.revision != input.revision {
            return Err(AuthError::Conflict);
        }
        let role = input.role.map(role_text).unwrap_or(&account.role);
        let disabled = input.disabled.unwrap_or(account.disabled);
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if account.role == "owner"
            && !account.disabled
            && (role != "owner" || disabled)
            && owners_in(&mut tx).await? <= 1
        {
            return Err(AuthError::InvalidInput(
                "The last enabled owner cannot be disabled or demoted.",
            ));
        }
        if !update_in(&mut tx, &account, role, disabled, input.revision).await? {
            return Err(AuthError::Conflict);
        }
        tx.commit().await?;
        view(
            by_id(&self.pool, id)
                .await?
                .ok_or(AuthError::Configuration)?,
        )
    }
    /// Native root profile/link service invokes this only after choosing/proving
    /// the canonical profile. HTTP callers cannot supply a canonical User ID.
    pub async fn bind_profile(
        &self,
        account_id: &str,
        canonical_user_id: &str,
        expected_revision: i64,
    ) -> Result<AccountView, AuthError> {
        if canonical_user_id.is_empty()
            || canonical_user_id.len() > 1024
            || canonical_user_id.contains('\0')
        {
            return Err(AuthError::InvalidInput("Invalid profile."));
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        bind_identity(&mut tx, "web", account_id, canonical_user_id).await?;
        if !bind_profile_in(&mut tx, account_id, canonical_user_id, expected_revision).await? {
            return Err(AuthError::Conflict);
        }
        tx.commit().await?;
        view(
            by_id(&self.pool, account_id)
                .await?
                .ok_or(AuthError::Configuration)?,
        )
    }
}
