use argon2::{
    password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
    Argon2, PasswordHash, PasswordVerifier,
};
use tokio::task::spawn_blocking;

use super::{AuthError, AuthService};

impl AuthService {
    pub(super) async fn password_hash(&self, password: String) -> Result<String, AuthError> {
        if password.len() > 1024 {
            return Err(AuthError::InvalidInput("Password is too long."));
        }
        let permit = self
            .workers
            .clone()
            .try_acquire_owned()
            .map_err(|_| AuthError::Busy)?;
        spawn_blocking(move || {
            let _permit = permit;
            Argon2::default()
                .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
                .map(|hash| hash.to_string())
                .map_err(|_| AuthError::Configuration)
        })
        .await
        .map_err(|_| AuthError::Configuration)?
    }
    pub(super) async fn password_matches(
        &self,
        password: String,
        hash: String,
    ) -> Result<bool, AuthError> {
        if password.len() > 1024 {
            return Ok(false);
        }
        let permit = self
            .workers
            .clone()
            .try_acquire_owned()
            .map_err(|_| AuthError::Busy)?;
        spawn_blocking(move || {
            let _permit = permit;
            Ok(PasswordHash::new(&hash).is_ok_and(|parsed| {
                Argon2::default()
                    .verify_password(password.as_bytes(), &parsed)
                    .is_ok()
            }))
        })
        .await
        .map_err(|_| AuthError::Configuration)?
    }
}
