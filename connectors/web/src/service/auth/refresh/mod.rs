use albert_core::db::repo::browser_refresh::renew;
use chrono::Utc;
use sha2::{Digest, Sha256};

use super::{AuthError, AuthService, LoginResult};
use crate::api::models::CredentialView;

pub(super) fn token_digest(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

impl AuthService {
    /// A bounded, stable opaque secret is independent for each browser login.
    /// Its database record and the underlying credential must both remain valid.
    pub async fn refresh(&self, token: &str) -> Result<LoginResult, AuthError> {
        if token.len() != 43
            || !token
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(AuthError::Unauthorized);
        }
        let now = Utc::now().timestamp();
        let renewed = renew(
            &self.pool,
            &token_digest(token),
            now,
            self.settings.credential_ttl_seconds as i64,
        )
        .await?
        .ok_or(AuthError::Unauthorized)?;
        let token = self.access_token(
            &renewed.account,
            &renewed.credential_id,
            &renewed.csrf,
            now,
            renewed.expires_at,
        )?;
        Ok(LoginResult {
            token,
            credential: CredentialView {
                id: renewed.credential_id,
                expires_at: renewed.expires_at,
            },
            csrf: renewed.csrf,
            refresh: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use albert_core::db::repo::account::by_id;
    use albert_core::db::Database;
    use chrono::Utc;
    use sqlx::{query, query_scalar, sqlite::SqlitePoolOptions};
    use tempfile::{tempdir, TempDir};
    use tokio::join;

    use super::{AuthError, AuthService};
    use crate::{
        api::models::CredentialUpdate, api::models::LoginRequest, service::auth::AuthSettings,
        service::auth::LoginResult,
    };

    async fn fixture() -> (TempDir, Arc<AuthService>) {
        let dir = tempdir().unwrap();
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        Database::from_pool(pool.clone()).await.unwrap();
        let auth = AuthService::open(pool, dir.path().join("key"), AuthSettings::default())
            .await
            .unwrap();
        (dir, auth)
    }
    async fn login(auth: &AuthService) -> LoginResult {
        auth.login(LoginRequest {
            username: "owner".into(),
            password: "owner".into(),
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn devices_and_tabs_refresh_independently_and_logout_only_revokes_one_device() {
        let (dir, auth) = fixture().await;
        let a = login(&auth).await;
        let b = login(&auth).await;
        let secret_a = &a.refresh.as_ref().unwrap().token;
        let secret_b = &b.refresh.as_ref().unwrap().token;
        assert_ne!(a.credential.id, b.credential.id);
        assert_ne!(secret_a, secret_b);
        let stored: String =
            query_scalar("SELECT token_digest FROM albert_browser_refresh WHERE credential_id=?")
                .bind(&a.credential.id)
                .fetch_one(&auth.pool)
                .await
                .unwrap();
        assert_ne!(&stored, secret_a);
        assert_eq!(stored.len(), 64);
        let (one, two, other) = join!(
            auth.refresh(secret_a),
            auth.refresh(secret_a),
            auth.refresh(secret_b)
        );
        let (one, two, other) = (one.unwrap(), two.unwrap(), other.unwrap());
        assert_eq!(one.credential.id, a.credential.id);
        assert_eq!(two.csrf, one.csrf);
        assert_eq!(one.csrf, a.csrf);
        auth.authenticate(&one.token).await.unwrap();
        auth.authenticate(&two.token).await.unwrap();
        auth.authenticate(&other.token).await.unwrap();
        let restarted = AuthService::open(
            auth.pool.clone(),
            dir.path().join("key"),
            AuthSettings::default(),
        )
        .await
        .unwrap();
        restarted.refresh(secret_b).await.unwrap();
        let principal = auth.authenticate(&one.token).await.unwrap();
        auth.revoke_credential(&principal, &one.credential.id)
            .await
            .unwrap();
        assert!(matches!(
            auth.refresh(secret_a).await,
            Err(AuthError::Unauthorized)
        ));
        assert!(matches!(
            auth.authenticate(&two.token).await,
            Err(AuthError::Unauthorized)
        ));
        auth.refresh(secret_b).await.unwrap();
    }

    #[tokio::test]
    async fn expired_access_can_renew_but_expired_refresh_and_disabled_accounts_cannot() {
        let (_dir, auth) = fixture().await;
        let login = login(&auth).await;
        let principal = auth.authenticate(&login.token).await.unwrap();
        let now = Utc::now().timestamp();
        query("UPDATE albert_browser_credentials SET issued_at=?,expires_at=? WHERE id=?")
            .bind(now - 200)
            .bind(now - 100)
            .bind(&login.credential.id)
            .execute(&auth.pool)
            .await
            .unwrap();
        assert!(matches!(
            auth.authenticate(&login.token).await,
            Err(AuthError::Unauthorized)
        ));
        let renewed = auth
            .refresh(&login.refresh.as_ref().unwrap().token)
            .await
            .unwrap();
        auth.authenticate(&renewed.token).await.unwrap();
        assert_eq!(renewed.csrf, principal.csrf());
        query("UPDATE albert_browser_refresh SET created_at=?,expires_at=? WHERE credential_id=?")
            .bind(now - 200)
            .bind(now - 1)
            .bind(&login.credential.id)
            .execute(&auth.pool)
            .await
            .unwrap();
        assert!(matches!(
            auth.refresh(&login.refresh.unwrap().token).await,
            Err(AuthError::Unauthorized)
        ));
        let second = login_helper(&auth).await;
        query("UPDATE albert_login_accounts SET disabled=1")
            .execute(&auth.pool)
            .await
            .unwrap();
        assert!(matches!(
            auth.refresh(&second.refresh.unwrap().token).await,
            Err(AuthError::Unauthorized)
        ));
    }

    async fn login_helper(auth: &AuthService) -> LoginResult {
        login(auth).await
    }

    #[tokio::test]
    async fn password_rotation_invalidates_all_previous_device_refresh_secrets() {
        let (_dir, auth) = fixture().await;
        let a = login(&auth).await;
        let b = login(&auth).await;
        let principal = auth.authenticate(&a.token).await.unwrap();
        let (_, updated) = auth
            .rotate_credentials(
                &principal,
                CredentialUpdate {
                    current_password: "owner".into(),
                    new_password: "new-private-fixture-password".into(),
                    new_username: None,
                },
            )
            .await
            .unwrap();
        for old in [a, b] {
            assert!(matches!(
                auth.refresh(&old.refresh.unwrap().token).await,
                Err(AuthError::Unauthorized)
            ));
        }
        auth.refresh(&updated.refresh.unwrap().token).await.unwrap();
    }
    #[tokio::test]
    async fn an_older_tab_access_token_stays_valid_after_a_newer_renewal() {
        let (_dir, auth) = fixture().await;
        let initial = login(&auth).await;
        let principal = auth.authenticate(&initial.token).await.unwrap();
        let account = by_id(&auth.pool, principal.account_id())
            .await
            .unwrap()
            .unwrap();
        let now = Utc::now().timestamp();
        query("UPDATE albert_browser_credentials SET issued_at=?,expires_at=? WHERE id=?")
            .bind(now - 60)
            .bind(now + 30)
            .bind(&initial.credential.id)
            .execute(&auth.pool)
            .await
            .unwrap();
        let older = auth
            .access_token(
                &account,
                &initial.credential.id,
                &initial.csrf,
                now - 30,
                now + 30,
            )
            .unwrap();
        let newer = auth.refresh(&initial.refresh.unwrap().token).await.unwrap();
        assert!(newer.credential.expires_at > now + 30);
        auth.authenticate(&newer.token).await.unwrap();
        auth.authenticate(&older).await.unwrap();
    }

    #[tokio::test]
    async fn refresh_racing_logout_cannot_restore_revoked_access_and_db_failure_is_not_unauthorized(
    ) {
        let (_dir, auth) = fixture().await;
        let login = login(&auth).await;
        let principal = auth.authenticate(&login.token).await.unwrap();
        let (renewed, revoked) = join!(
            auth.refresh(&login.refresh.as_ref().unwrap().token),
            auth.revoke_credential(&principal, &login.credential.id)
        );
        revoked.unwrap();
        if let Ok(renewed) = renewed {
            assert!(matches!(
                auth.authenticate(&renewed.token).await,
                Err(AuthError::Unauthorized)
            ));
        }
        assert!(matches!(
            auth.refresh("not-a-token").await,
            Err(AuthError::Unauthorized)
        ));
        auth.pool.close().await;
        assert!(matches!(
            auth.refresh(&login.refresh.unwrap().token).await,
            Err(AuthError::Database(_))
        ));
    }
}
