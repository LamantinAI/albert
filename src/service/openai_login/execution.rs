use std::{fs::DirBuilder, os::unix::fs::DirBuilderExt, sync::Arc};

use tracing::warn;

use super::OpenAiLoginService;
use crate::{
    api::{models::OpenAiLoginPhase, principal::Principal},
    openai_login::LoginFlow,
};

impl OpenAiLoginService {
    pub(super) async fn exchange_and_install(
        self: Arc<Self>,
        caller: Principal,
        id: String,
        flow: LoginFlow,
        code: String,
    ) {
        // The outbound request holds neither the attempt/commit fence nor the file
        // gate. Cancellation can win while the provider is replying.
        let result = flow.exchange(&code).await;
        drop(code);
        drop(flow);
        let mut attempts = self.attempts.lock().await;
        let Ok(attempt) = Self::find(&mut attempts, &caller, &id) else {
            return;
        };
        if !matches!(attempt.view.phase, OpenAiLoginPhase::Exchanging) {
            return;
        }
        let valid = self
            .auth
            .validate_principal(&caller)
            .await
            .is_ok_and(|current| current.owner_permit().is_ok());
        if !valid {
            attempt.finish(
                OpenAiLoginPhase::Failed,
                "Sign-in credentials changed. Authorize again with a current owner session.",
            );
            return;
        }
        let tokens = match result {
            Ok(tokens) => tokens,
            Err(error) => {
                warn!(
                    category = error.category(),
                    http_status = error.http_status(),
                    oauth_code = error.oauth_code(),
                    "subscription authorization exchange failed"
                );
                attempt.finish(OpenAiLoginPhase::Failed, error.public_message());
                return;
            }
        };
        if self.runtime.subscription.path().exists() && !attempt.replace {
            attempt.finish(OpenAiLoginPhase::Failed, "Subscription credentials appeared during authorization. Start again and confirm replacement.");
            return;
        }
        let _credentials = self.credential_gate.lock().await;
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
