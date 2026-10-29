use chrono::Utc;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use super::{random_id, role, validate_username, view, AuthError, AuthService, LoginResult};
use crate::{
    api::{
        models::{AccountRole, AccountView, CredentialUpdate, CredentialView, LoginRequest},
        principal::Principal,
    },
    db::{
        account::LoginAccount,
        repo::account::{by_id, by_name, credential, issue, revoke, rotate},
    },
};

#[derive(Clone, Serialize, Deserialize)]
struct Claims {
    sub: String,
    jti: String,
    v: i64,
    role: AccountRole,
    must_change_password: bool,
    csrf: String,
    exp: i64,
    iat: i64,
    nbf: i64,
    iss: String,
    aud: String,
}

impl AuthService {
    pub async fn login(&self, input: LoginRequest) -> Result<LoginResult, AuthError> {
        if !self.admit_login() {
            return Err(AuthError::Busy);
        }
        validate_username(&input.username).map_err(|_| AuthError::InvalidCredentials)?;
        let account = by_name(&self.pool, &input.username)
            .await?
            .ok_or(AuthError::InvalidCredentials)?;
        if account.disabled
            || !self
                .password_matches(input.password, account.password_hash.clone())
                .await?
        {
            return Err(AuthError::InvalidCredentials);
        }
        self.issue_account(account).await
    }
    async fn issue_account(&self, account: LoginAccount) -> Result<LoginResult, AuthError> {
        let now = Utc::now().timestamp();
        let id = random_id();
        let expires = now + self.settings.credential_ttl_seconds as i64;
        let claims = Claims {
            sub: account.id.clone(),
            jti: id.clone(),
            v: account.token_version,
            role: role(&account.role)?,
            must_change_password: account.must_change_password,
            csrf: random_id(),
            exp: expires,
            iat: now,
            nbf: now,
            iss: "albert".into(),
            aud: "albert-web".into(),
        };
        let token = encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(&self.key),
        )
        .map_err(|_| AuthError::Configuration)?;
        if !issue(&self.pool, &id, &account, now, expires).await? {
            return Err(AuthError::Conflict);
        }
        Ok(LoginResult {
            token,
            credential: CredentialView {
                id,
                expires_at: expires,
            },
        })
    }
    pub async fn authenticate(&self, token: &str) -> Result<Principal, AuthError> {
        if token.len() > 8192 {
            return Err(AuthError::Unauthorized);
        }
        let mut validation = Validation::new(Algorithm::HS256);
        validation.leeway = 0;
        validation.validate_nbf = true;
        validation.set_required_spec_claims(&["exp", "sub", "iss", "aud", "nbf", "iat"]);
        validation.set_issuer(&["albert"]);
        validation.set_audience(&["albert-web"]);
        let claims = decode::<Claims>(token, &DecodingKey::from_secret(&self.key), &validation)
            .map_err(|_| AuthError::Unauthorized)?
            .claims;
        let now = Utc::now().timestamp();
        if claims.iat > now
            || claims.exp <= claims.iat
            || claims.exp - claims.iat > 86400
            || claims.csrf.len() < 32
        {
            return Err(AuthError::Unauthorized);
        }
        let account = by_id(&self.pool, &claims.sub)
            .await?
            .ok_or(AuthError::Unauthorized)?;
        let stored = credential(&self.pool, &claims.jti)
            .await?
            .ok_or(AuthError::Unauthorized)?;
        if account.disabled
            || account.token_version != claims.v
            || role(&account.role)? != claims.role
            || account.must_change_password != claims.must_change_password
            || stored.account_id != claims.sub
            || stored.id != claims.jti
            || stored.token_version != claims.v
            || stored.revoked_at.is_some()
            || stored.issued_at != claims.iat
            || stored.expires_at != claims.exp
            || stored.expires_at <= now
        {
            return Err(AuthError::Unauthorized);
        }
        Ok(Principal::authenticated(
            view(account)?,
            claims.jti,
            claims.csrf,
        ))
    }
    pub async fn revoke_credential(
        &self,
        principal: &Principal,
        id: &str,
    ) -> Result<(), AuthError> {
        // A credential never grants authority over another account's browser.
        revoke(
            &self.pool,
            id,
            principal.account_id(),
            Utc::now().timestamp(),
        )
        .await?;
        Ok(())
    }
    /// Recheck a previously authenticated native principal (for a long-lived
    /// event stream) without accepting account/role fields from the browser.
    pub async fn validate_principal(&self, principal: &Principal) -> Result<Principal, AuthError> {
        let account = by_id(&self.pool, principal.account_id())
            .await?
            .ok_or(AuthError::Unauthorized)?;
        let stored = credential(&self.pool, principal.credential_id())
            .await?
            .ok_or(AuthError::Unauthorized)?;
        if account.disabled
            || stored.account_id != account.id
            || stored.revoked_at.is_some()
            || stored.token_version != account.token_version
            || stored.expires_at <= Utc::now().timestamp()
            || role(&account.role)? != principal.role()
            || account.must_change_password != principal.must_rotate()
        {
            return Err(AuthError::Unauthorized);
        }
        Ok(Principal::authenticated(
            view(account)?,
            principal.credential_id().into(),
            principal.csrf().into(),
        ))
    }
    pub async fn rotate_credentials(
        &self,
        principal: &Principal,
        input: CredentialUpdate,
    ) -> Result<(AccountView, LoginResult), AuthError> {
        if !self.admit_login() {
            return Err(AuthError::Busy);
        }
        if !(12..=1024).contains(&input.new_password.len())
            || input.new_password == input.current_password
        {
            return Err(AuthError::InvalidInput(
                "Choose a different password containing 12–1024 bytes.",
            ));
        }
        let account = by_id(&self.pool, principal.account_id())
            .await?
            .ok_or(AuthError::Unauthorized)?;
        if account.disabled
            || !self
                .password_matches(input.current_password, account.password_hash.clone())
                .await?
        {
            return Err(AuthError::InvalidCredentials);
        }
        let username = input
            .new_username
            .unwrap_or_else(|| account.username.clone());
        validate_username(&username)?;
        let hash = self.password_hash(input.new_password).await?;
        if !rotate(&self.pool, &account, &username, &hash).await? {
            return Err(AuthError::Conflict);
        }
        let account = by_id(&self.pool, &account.id)
            .await?
            .ok_or(AuthError::Unauthorized)?;
        let response = view(account.clone())?;
        Ok((response, self.issue_account(account).await?))
    }
}
