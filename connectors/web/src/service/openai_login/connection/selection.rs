use std::{sync::Arc, time::Duration};

use tokio::{spawn, time::timeout};

use super::{stale, OpenAiLoginService};
use crate::{
    api::errors::ApiError, api::errors::ApiResult, api::models::SubscriptionApplyRequest,
    api::models::SubscriptionCatalogModel, api::models::SubscriptionConnectionView,
    api::models::SubscriptionPhase, api::models::SubscriptionSelection, api::principal::Principal,
};

impl OpenAiLoginService {
    pub async fn select_subscription(
        self: &Arc<Self>,
        caller: &Principal,
        input: SubscriptionSelection,
    ) -> ApiResult<SubscriptionConnectionView> {
        self.auth.validate_principal(caller).await?.owner_permit()?;
        let (_, generation) = self.current_subscription().await?;
        if input.generation != generation {
            return Err(stale());
        }
        let mut attempts = self.attempts.lock().await;
        for attempt in attempts.iter_mut() {
            attempt.expire();
        }
        if attempts.iter().any(|attempt| attempt.active()) {
            return Err(ApiError::conflict(
                "Complete or cancel sign-in before selecting a model.",
            ));
        }
        let _credentials = self.credential_gate.lock().await;
        let mut connection = self.connection.lock().await;
        if connection.generation != input.generation {
            return Err(stale());
        }
        if connection.applying() {
            return Err(ApiError::conflict(
                "Model settings are already being applied.",
            ));
        }
        let snapshot = connection
            .snapshot
            .as_ref()
            .filter(|snapshot| snapshot.revision == input.catalog_revision)
            .ok_or_else(stale)?;
        let model = snapshot
            .models
            .iter()
            .find(|model| model.slug == input.slug && model.selectable)
            .cloned()
            .ok_or_else(|| {
                ApiError::invalid(
                    "Choose an available model from the current subscription catalog.",
                )
            })?;
        self.auth.validate_principal(caller).await?.owner_permit()?;
        connection.selected_id = None;
        connection.selected_slug = Some(input.slug);
        connection.phase = SubscriptionPhase::Applying;
        connection.message = Some("Saving the selected model and applying its settings.".into());
        let view = connection.view();
        drop(connection);
        drop(_credentials);
        drop(attempts);
        let service = Arc::clone(self);
        let caller = caller.clone();
        spawn(async move {
            service
                .commit_subscription_selection(caller, generation, model, input.prefer)
                .await;
        });
        Ok(view)
    }
    async fn commit_subscription_selection(
        self: Arc<Self>,
        caller: Principal,
        generation: String,
        model: SubscriptionCatalogModel,
        prefer: bool,
    ) {
        // The detached worker owns every commit lease, including while a blocking
        // file transaction is running. Dropping the HTTP request cannot release it.
        let _attempts = self.attempts.lock().await;
        let _credentials = self.credential_gate.lock().await;
        let connection = self.connection.lock().await;
        if connection.generation != generation || !connection.applying() {
            return;
        }
        drop(connection);
        let valid = self
            .auth
            .validate_principal(&caller)
            .await
            .is_ok_and(|current| current.owner_permit().is_ok());
        let result = if valid {
            self.models
                .connect_subscription(model, self.model_origin.clone(), prefer)
                .await
        } else {
            Err(ApiError::forbidden())
        };
        let mut connection = self.connection.lock().await;
        if connection.generation != generation || !connection.applying() {
            return;
        }
        match result {
            Ok((id, preferred)) => {
                connection.selected_id = Some(id);
                connection.preferred = preferred;
            }
            Err(_) => {
                connection.phase = SubscriptionPhase::ApplyFailed;
                connection.message = Some("Credentials remain connected. Model settings could not be saved; choose the model again to retry.".into());
                return;
            }
        }
        drop(connection);
        drop(_credentials);
        drop(_attempts);
        self.apply_subscription_generation(generation).await;
    }
    pub async fn retry_subscription_apply(
        self: &Arc<Self>,
        caller: &Principal,
        input: SubscriptionApplyRequest,
    ) -> ApiResult<SubscriptionConnectionView> {
        self.auth.validate_principal(caller).await?.owner_permit()?;
        let mut attempts = self.attempts.lock().await;
        for attempt in attempts.iter_mut() {
            attempt.expire();
        }
        if attempts.iter().any(|attempt| attempt.active()) {
            return Err(ApiError::conflict(
                "Complete or cancel sign-in before applying model settings.",
            ));
        }
        let mut connection = self.connection.lock().await;
        if connection.generation != input.generation {
            return Err(stale());
        }
        if !matches!(connection.phase, SubscriptionPhase::ApplyFailed)
            || connection.selected_id.is_none()
        {
            return Err(ApiError::conflict(
                "Select a catalog model before applying its settings.",
            ));
        }
        connection.phase = SubscriptionPhase::Applying;
        let view = connection.view();
        drop(connection);
        drop(attempts);
        let service = Arc::clone(self);
        spawn(async move {
            service
                .apply_subscription_generation(input.generation)
                .await;
        });
        Ok(view)
    }
    async fn apply_subscription_generation(&self, generation: String) {
        let ready = matches!(
            timeout(Duration::from_secs(30), self.runtime.activate()).await,
            Ok(Ok(()))
        ) && self.runtime.status().model_ready;
        let mut connection = self.connection.lock().await;
        if connection.generation != generation || !connection.applying() {
            return;
        }
        connection.phase = if ready {
            SubscriptionPhase::Ready
        } else {
            SubscriptionPhase::ApplyFailed
        };
        connection.message = Some(if ready { "Selected model settings applied." }
            else { "Credentials and selected model settings are saved. Runtime application failed; retry Apply." }.into());
    }
}
