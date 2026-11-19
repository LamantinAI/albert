use std::{fs::DirBuilder, os::unix::fs::DirBuilderExt, sync::Arc};

use albert_core::openai_login::{DeviceAuthEndpoints, DeviceLogin, ExchangeError};
use octo_openai_auth::Tokens;
use tokio::time::Instant;
use tracing::warn;

use super::OpenAiLoginService;
use crate::{api::models::OpenAiLoginPhase, api::principal::Principal};

impl OpenAiLoginService {
    pub(super) async fn authorize_device(
        self: Arc<Self>,
        caller: Principal,
        id: String,
        endpoints: DeviceAuthEndpoints,
        deadline: Instant,
    ) {
        let result = match DeviceLogin::start(&endpoints, deadline).await {
            Ok(login) => {
                let mut attempts = self.attempts.lock().await;
                let Ok(attempt) = Self::find(&mut attempts, &caller, &id) else {
                    return;
                };
                if !matches!(attempt.view.phase, OpenAiLoginPhase::Preparing) {
                    return;
                }
                attempt.view.phase = OpenAiLoginPhase::AwaitingDeviceCode;
                attempt.view.verification_url = Some(login.verification_url().into());
                attempt.view.user_code = Some(login.user_code().into());
                drop(attempts);
                login.complete().await
            }
            Err(error) => Err(error),
        };
        self.install_result(&caller, &id, result).await;
    }

    async fn install_result(
        &self,
        caller: &Principal,
        id: &str,
        result: Result<Tokens, ExchangeError>,
    ) {
        // The outbound requests hold neither the attempt fence nor the credential gate.
        let mut attempts = self.attempts.lock().await;
        let Ok(attempt) = Self::find(&mut attempts, caller, id) else {
            return;
        };
        if !attempt.active() {
            return;
        }
        let _credentials = self.credential_gate.lock().await;
        let valid = self
            .auth
            .validate_principal(caller)
            .await
            .is_ok_and(|current| current.owner_permit().is_ok());
        if !valid {
            attempt.finish(
                OpenAiLoginPhase::Failed,
                "Sign-in credentials changed. Authorize again with a current owner session.",
            );
            return;
        }
        attempt.expire();
        if !attempt.active() {
            return;
        }
        let tokens = match result {
            Ok(tokens) => tokens,
            Err(error) => {
                warn!(
                    category = error.category(),
                    http_status = error.http_status(),
                    oauth_code = error.oauth_code(),
                    "subscription device authorization failed"
                );
                let phase = if error.expired() {
                    OpenAiLoginPhase::Expired
                } else {
                    OpenAiLoginPhase::Failed
                };
                attempt.finish(phase, error.public_message());
                return;
            }
        };
        if self.runtime.subscription.path().exists() && !attempt.replace {
            attempt.finish(OpenAiLoginPhase::Failed, "Subscription credentials appeared during authorization. Start again and confirm replacement.");
            return;
        }
        let Some(parent) = self.runtime.subscription.path().parent() else {
            attempt.finish(
                OpenAiLoginPhase::Failed,
                "The credential store is unavailable.",
            );
            return;
        };
        let tokens_account = tokens.account_id.clone();
        if DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)
            .is_err()
            || self.runtime.subscription.install(tokens).await.is_err()
        {
            attempt.finish(
                OpenAiLoginPhase::Failed,
                "The credential store could not be updated. Existing credentials were preserved.",
            );
            return;
        }
        self.connection.lock().await.installed(tokens_account);
        attempt.view.credentials_saved = true;
        attempt.finish(
            OpenAiLoginPhase::Connected,
            "Subscription credentials connected. Choose a model from your account catalog.",
        );
    }
}
