mod credentials;
mod keys;
mod password;
mod users;

use std::{
    collections::VecDeque,
    io::Error as IoError,
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::{rngs::OsRng, RngCore};
use sqlx::{Error as SqlxError, SqlitePool};
use thiserror::Error;
use tokio::sync::Semaphore;

use self::keys::load_or_create;
use crate::{
    api::models::{AccountRole, AccountView, CredentialView},
    db::{account::LoginAccount, error::DatabaseError, repo::account::seed_owner},
};

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("Invalid credentials.")]
    InvalidCredentials,
    #[error("Authentication is required.")]
    Unauthorized,
    #[error("Action is not permitted.")]
    Forbidden,
    #[error("Authentication is busy. Try again shortly.")]
    Busy,
    #[error("{0}")]
    InvalidInput(&'static str),
    #[error("Account changed. Reload and retry.")]
    Conflict,
    #[error("Authentication configuration is unavailable.")]
    Configuration,
    #[error("Authentication database is unavailable.")]
    Database(#[from] SqlxError),
    #[error("Authentication key is unavailable.")]
    Io(#[from] IoError),
}

impl From<DatabaseError> for AuthError {
    fn from(error: DatabaseError) -> Self {
        match error {
            DatabaseError::IdentityConflict | DatabaseError::RevisionConflict => Self::Conflict,
            DatabaseError::Sqlx(error) => Self::Database(error),
            _ => Self::Configuration,
        }
    }
}

pub struct LoginResult {
    pub token: String,
    pub credential: CredentialView,
}

#[derive(Clone)]
pub struct AuthSettings {
    pub public_origin: String,
    pub credential_ttl_seconds: u64,
    pub secure_cookie: bool,
    pub login_attempts: usize,
    pub login_window_seconds: u64,
    pub password_workers: usize,
}
impl Default for AuthSettings {
    fn default() -> Self {
        Self {
            public_origin: String::new(),
            credential_ttl_seconds: 3600,
            secure_cookie: false,
            login_attempts: 10,
            login_window_seconds: 60,
            password_workers: 2,
        }
    }
}
pub struct AuthService {
    pub(super) pool: SqlitePool,
    pub(super) key: Vec<u8>,
    pub(super) settings: AuthSettings,
    pub(super) workers: Arc<Semaphore>,
    attempts: Mutex<VecDeque<Instant>>,
}
impl AuthService {
    pub async fn open(
        pool: SqlitePool,
        key_path: impl AsRef<Path>,
        settings: AuthSettings,
    ) -> Result<Arc<Self>, AuthError> {
        if !(60..=86400).contains(&settings.credential_ttl_seconds)
            || settings.password_workers == 0
            || settings.login_attempts == 0
            || settings.login_window_seconds == 0
        {
            return Err(AuthError::Configuration);
        }
        let key = load_or_create(key_path.as_ref())?;
        let service = Arc::new(Self {
            pool,
            key,
            workers: Arc::new(Semaphore::new(settings.password_workers)),
            settings,
            attempts: Mutex::new(VecDeque::new()),
        });
        // No model/provider/TG dependency. The transactional singleton prevents
        // restart or concurrent initialization from restoring default credentials.
        let hash = service.password_hash("owner".into()).await?;
        seed_owner(&service.pool, &hash).await?;
        Ok(service)
    }
    pub fn settings(&self) -> &AuthSettings {
        &self.settings
    }
    pub(super) fn admit_login(&self) -> bool {
        let mut attempts = self.attempts.lock().unwrap_or_else(|e| e.into_inner());
        let window = Duration::from_secs(self.settings.login_window_seconds);
        while attempts
            .front()
            .is_some_and(|time| time.elapsed() >= window)
        {
            attempts.pop_front();
        }
        if attempts.len() >= self.settings.login_attempts {
            return false;
        }
        attempts.push_back(Instant::now());
        true
    }
}
pub(super) fn random_id() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}
pub(super) fn role(value: &str) -> Result<AccountRole, AuthError> {
    match value {
        "owner" => Ok(AccountRole::Owner),
        "user" => Ok(AccountRole::User),
        _ => Err(AuthError::Configuration),
    }
}
pub(super) fn role_text(value: AccountRole) -> &'static str {
    match value {
        AccountRole::Owner => "owner",
        AccountRole::User => "user",
    }
}
pub(super) fn view(account: LoginAccount) -> Result<AccountView, AuthError> {
    Ok(AccountView {
        role: role(&account.role)?,
        id: account.id,
        username: account.username,
        canonical_user_id: account.canonical_user_id,
        must_change_password: account.must_change_password,
        disabled: account.disabled,
        revision: account.revision,
    })
}
pub(super) fn validate_username(value: &str) -> Result<(), AuthError> {
    if !(3..=64).contains(&value.len())
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_.@-".contains(&c))
    {
        return Err(AuthError::InvalidInput(
            "Username must contain 3–64 letters, digits, _, ., @ or -.",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
    use serde_json::{from_slice, json, Value};
    use sqlx::{query, sqlite::SqlitePoolOptions};
    use tempfile::{tempdir, TempDir};

    use super::{AuthService, AuthSettings, Engine, URL_SAFE_NO_PAD};
    use crate::{
        api::models::{AccountRole, CredentialUpdate, LoginRequest, UserCreate, UserUpdate},
        db::Database,
    };

    async fn fixture() -> (TempDir, Arc<AuthService>) {
        let dir = tempdir().unwrap();
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        Database::from_pool(pool.clone()).await.unwrap();
        let settings = AuthSettings {
            public_origin: "http://localhost:9877".into(),
            ..Default::default()
        };
        let auth = AuthService::open(pool, dir.path().join("signing.key"), settings)
            .await
            .unwrap();
        (dir, auth)
    }
    async fn rotated(auth: &AuthService) -> super::LoginResult {
        let initial = auth
            .login(LoginRequest {
                username: "owner".into(),
                password: "owner".into(),
            })
            .await
            .unwrap();
        let caller = auth.authenticate(&initial.token).await.unwrap();
        auth.rotate_credentials(
            &caller,
            CredentialUpdate {
                current_password: "owner".into(),
                new_password: "changed-owner-password".into(),
                new_username: None,
            },
        )
        .await
        .unwrap()
        .1
    }

    #[tokio::test]
    async fn owner_seed_requires_rotation_is_profileless_and_never_resets_on_restart() {
        let (dir, auth) = fixture().await;
        let original = auth
            .login(LoginRequest {
                username: "owner".into(),
                password: "owner".into(),
            })
            .await
            .unwrap();
        let caller = auth.authenticate(&original.token).await.unwrap();
        assert!(caller.must_rotate());
        assert!(caller.canonical_user_id().is_none());
        assert!(caller.owner_permit().is_err());
        let (_, new) = auth
            .rotate_credentials(
                &caller,
                CredentialUpdate {
                    current_password: "owner".into(),
                    new_password: "changed-owner-password".into(),
                    new_username: Some("changed-owner".into()),
                },
            )
            .await
            .unwrap();
        assert!(auth.authenticate(&original.token).await.is_err());
        let caller = auth.authenticate(&new.token).await.unwrap();
        assert!(!caller.must_rotate());
        assert!(caller.owner_permit().is_ok());
        let restarted = AuthService::open(
            auth.pool.clone(),
            dir.path().join("signing.key"),
            auth.settings.clone(),
        )
        .await
        .unwrap();
        assert!(restarted
            .login(LoginRequest {
                username: "owner".into(),
                password: "owner".into()
            })
            .await
            .is_err());
        assert!(restarted.authenticate(&new.token).await.is_ok());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                dir.path()
                    .join("signing.key")
                    .metadata()
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[tokio::test]
    async fn revoked_credentials_and_current_db_role_disabled_rotation_are_rechecked() {
        let (_dir, auth) = fixture().await;
        let login = rotated(&auth).await;
        let caller = auth.authenticate(&login.token).await.unwrap();
        let permit = caller.owner_permit().unwrap();
        let user = auth
            .create_user(
                &permit,
                UserCreate {
                    username: "normal-user".into(),
                    password: "new-user-password".into(),
                    role: AccountRole::User,
                },
            )
            .await
            .unwrap();
        assert!(user.canonical_user_id.is_some());
        let login_user = auth
            .login(LoginRequest {
                username: "normal-user".into(),
                password: "new-user-password".into(),
            })
            .await
            .unwrap();
        let user_principal = auth.authenticate(&login_user.token).await.unwrap();
        assert!(user_principal.owner_permit().is_err());
        auth.update_user(
            &permit,
            &user.id,
            UserUpdate {
                revision: user.revision,
                role: None,
                disabled: Some(true),
            },
        )
        .await
        .unwrap();
        assert!(auth.authenticate(&login_user.token).await.is_err());
        assert!(auth.validate_principal(&user_principal).await.is_err());
        assert!(auth
            .update_user(
                &permit,
                caller.account_id(),
                UserUpdate {
                    revision: caller.account().revision,
                    role: Some(AccountRole::User),
                    disabled: None
                }
            )
            .await
            .is_err());
        auth.revoke_credential(&caller, caller.credential_id())
            .await
            .unwrap();
        assert!(auth.authenticate(&login.token).await.is_err());
    }

    #[tokio::test]
    async fn signed_tokens_still_require_issuer_audience_expiry_and_current_database_state() {
        let (_dir, auth) = fixture().await;
        let login = rotated(&auth).await;
        let payload = URL_SAFE_NO_PAD
            .decode(login.token.split('.').nth(1).unwrap())
            .unwrap();
        let claims: Value = from_slice(&payload).unwrap();
        for (field, value) in [
            ("iss", json!("another-host")),
            ("aud", json!("another-client")),
            ("exp", json!(1)),
        ] {
            let mut changed = claims.clone();
            changed[field] = value;
            let token = encode(
                &Header::new(Algorithm::HS256),
                &changed,
                &EncodingKey::from_secret(&auth.key),
            )
            .unwrap();
            assert!(auth.authenticate(&token).await.is_err());
        }
        let token = encode(
            &Header::new(Algorithm::HS512),
            &claims,
            &EncodingKey::from_secret(&auth.key),
        )
        .unwrap();
        assert!(auth.authenticate(&token).await.is_err());
        query("UPDATE albert_login_accounts SET role='user' WHERE id='bootstrap-owner'")
            .execute(&auth.pool)
            .await
            .unwrap();
        assert!(auth.authenticate(&login.token).await.is_err());
        query("UPDATE albert_login_accounts SET role='owner',must_change_password=1 WHERE id='bootstrap-owner'").execute(&auth.pool).await.unwrap();
        assert!(auth.authenticate(&login.token).await.is_err());
    }

    #[tokio::test]
    async fn profile_binding_is_explicit_cas_and_does_not_reassign_a_populated_account() {
        let (_dir, auth) = fixture().await;
        let login = rotated(&auth).await;
        let caller = auth.authenticate(&login.token).await.unwrap();
        let bound = auth
            .bind_profile(
                caller.account_id(),
                "existing-tg-user",
                caller.account().revision,
            )
            .await
            .unwrap();
        assert_eq!(bound.canonical_user_id.as_deref(), Some("existing-tg-user"));
        assert!(auth
            .bind_profile(caller.account_id(), "different-user", bound.revision)
            .await
            .is_err());
        assert!(auth
            .bind_profile(
                caller.account_id(),
                "existing-tg-user",
                caller.account().revision
            )
            .await
            .is_err());
        assert_eq!(
            auth.authenticate(&login.token)
                .await
                .unwrap()
                .canonical_user_id(),
            Some("existing-tg-user")
        );
    }

    #[test]
    fn signing_key_publication_is_complete_and_concurrent_initializers_share_it() {
        use super::load_or_create;
        use std::{sync::Barrier, thread::scope};
        let dir = tempdir().unwrap();
        let path = dir.path().join("signing.key");
        let barrier = Arc::new(Barrier::new(2));
        scope(|scope| {
            let mut handles = Vec::new();
            for _ in 0..2 {
                let barrier = barrier.clone();
                let path = &path;
                handles.push(scope.spawn(move || {
                    barrier.wait();
                    load_or_create(path).unwrap()
                }));
            }
            let left = handles.remove(0).join().unwrap();
            let right = handles.remove(0).join().unwrap();
            assert_eq!(left, right);
            assert_eq!(left.len(), 32);
        });
    }
}
