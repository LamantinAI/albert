mod selection;

use octo_openai_auth::Subscription;
use rand::random;

use super::{catalog::fetch, catalog::CatalogError, OpenAiLoginService};
use crate::{
    api::errors::ApiError, api::errors::ApiResult, api::models::SubscriptionCatalogView,
    api::models::SubscriptionConnectionView, api::models::SubscriptionPhase,
    api::principal::Principal,
};

pub(super) struct ConnectionState {
    generation: String,
    account: Option<String>,
    snapshot: Option<SubscriptionCatalogView>,
    phase: SubscriptionPhase,
    selected_id: Option<String>,
    selected_slug: Option<String>,
    preferred: Option<String>,
    message: Option<String>,
}
impl ConnectionState {
    pub(super) fn new() -> Self {
        Self {
            generation: new_generation(),
            account: None,
            snapshot: None,
            phase: SubscriptionPhase::Disconnected,
            selected_id: None,
            selected_slug: None,
            preferred: None,
            message: None,
        }
    }
    pub(super) fn installed(&mut self, account: String) {
        *self = Self::new();
        self.account = Some(account);
        self.phase = SubscriptionPhase::Connected;
    }
    pub(super) fn applying(&self) -> bool {
        matches!(self.phase, SubscriptionPhase::Applying)
    }
    fn view(&self) -> SubscriptionConnectionView {
        SubscriptionConnectionView {
            connected: self.account.is_some(),
            generation: self.account.as_ref().map(|_| self.generation.clone()),
            phase: self.phase.clone(),
            selected_model_id: self.selected_id.clone(),
            selected_slug: self.selected_slug.clone(),
            preferred_model_id: self.preferred.clone(),
            message: self.message.clone(),
        }
    }
}
fn new_generation() -> String {
    format!("{:032x}", random::<u128>())
}
impl OpenAiLoginService {
    async fn current_subscription(&self) -> ApiResult<(Subscription, String)> {
        let _credentials = self.credential_gate.lock().await;
        let subscription = self.runtime.subscription.fresh().await.map_err(|_| ApiError::unavailable(
            "Saved subscription credentials could not be loaded or refreshed. Retry, or reconnect the subscription."))?;
        let mut connection = self.connection.lock().await;
        if connection.account.as_deref() != Some(subscription.account_id.as_str()) {
            connection.installed(subscription.account_id.clone());
        }
        Ok((subscription, connection.generation.clone()))
    }
    pub async fn subscription_connection(
        &self,
        caller: &Principal,
    ) -> ApiResult<SubscriptionConnectionView> {
        self.auth.validate_principal(caller).await?.owner_permit()?;
        if self.runtime.subscription.path().exists() {
            let applying = {
                let connection = self.connection.lock().await;
                connection.applying().then(|| connection.view())
            };
            if let Some(view) = applying {
                self.auth.validate_principal(caller).await?.owner_permit()?;
                return Ok(view);
            }
            self.current_subscription().await?;
        } else {
            let _credentials = self.credential_gate.lock().await;
            // Recheck beneath the install lease before clearing stale metadata.
            if !self.runtime.subscription.path().exists() {
                let mut connection = self.connection.lock().await;
                if connection.account.is_some() {
                    *connection = ConnectionState::new();
                }
            }
        }
        self.auth.validate_principal(caller).await?.owner_permit()?;
        Ok(self.connection.lock().await.view())
    }
    pub async fn subscription_catalog(
        &self,
        caller: &Principal,
    ) -> ApiResult<SubscriptionCatalogView> {
        self.auth.validate_principal(caller).await?.owner_permit()?;
        let (subscription, generation) = self.current_subscription().await?;
        let result = fetch(&self.model_origin, &subscription).await;
        let result = if matches!(result, Err(CatalogError::Unauthorized)) {
            // A single 401 escalation uses the same refresh owner as inference.
            // Replacement cannot race this refresh/generation check.
            let _credentials = self.credential_gate.lock().await;
            if self.connection.lock().await.generation != generation {
                return Err(stale());
            }
            let subscription = self
                .runtime
                .subscription
                .force_refresh()
                .await
                .map_err(|_| ApiError::unavailable(CatalogError::Unauthorized.message()))?;
            {
                let mut connection = self.connection.lock().await;
                if connection.account.as_deref() != Some(subscription.account_id.as_str()) {
                    connection.installed(subscription.account_id.clone());
                    return Err(stale());
                }
            }
            drop(_credentials);
            fetch(&self.model_origin, &subscription).await
        } else {
            result
        };
        let models = result.map_err(|error| ApiError::unavailable(error.message()))?;
        self.auth.validate_principal(caller).await?.owner_permit()?;
        let mut connection = self.connection.lock().await;
        if connection.generation != generation {
            return Err(stale());
        }
        let view = SubscriptionCatalogView {
            generation,
            revision: new_generation(),
            models,
        };
        connection.snapshot = Some(view.clone());
        Ok(view)
    }
}
fn stale() -> ApiError {
    ApiError::conflict("The subscription connection or model catalog changed. Reload the model list and choose again.")
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap, fs::create_dir_all, sync::atomic::AtomicUsize,
        sync::atomic::Ordering, sync::Arc, time::Duration,
    };

    use axum::{
        extract::Query, http::HeaderMap, http::StatusCode, routing::get, serve, Json, Router,
    };
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use octo_openai_auth::Tokens;
    use serde_json::{json, to_string, Value};
    use tokio::{net::TcpListener, spawn, sync::Notify, task::yield_now, time::timeout};

    use crate::{
        api::models::SubscriptionApplyRequest, api::models::SubscriptionCatalogView,
        api::models::SubscriptionPhase, api::models::SubscriptionSelection,
        service::openai_login::tests::fixture, service::openai_login::OpenAiLoginService,
    };

    async fn install(service: &OpenAiLoginService, account: &str) {
        create_dir_all(service.runtime.subscription.path().parent().unwrap()).unwrap();
        let payload = URL_SAFE_NO_PAD.encode(json!({"exp":9_999_999_999_i64,"https://api.openai.com/auth":{"chatgpt_account_id":account}}).to_string());
        service
            .runtime
            .subscription
            .install(Tokens {
                access_token: format!("x.{payload}.x"),
                refresh_token: "synthetic-refresh".into(),
                id_token: String::new(),
                account_id: account.into(),
            })
            .await
            .unwrap();
    }
    fn models() -> Value {
        json!({"models":[{"slug":"account-model","display_name":"Account Model","description":"An account-specific description.","visibility":"list","supported_in_api":false,"input_modalities":["text"],"context_window":32000,"tool_mode":"direct","base_instructions":"private-catalog-prompt"}]})
    }
    fn selection(catalog: &SubscriptionCatalogView) -> SubscriptionSelection {
        SubscriptionSelection {
            generation: catalog.generation.clone(),
            catalog_revision: catalog.revision.clone(),
            slug: "account-model".into(),
            prefer: false,
        }
    }

    #[tokio::test]
    async fn catalog_retry_selection_generation_and_restart_preserve_connection() {
        timeout(Duration::from_secs(8), async {
            let (_dir, mut service, owner) = fixture().await;
            install(&service, "synthetic-account").await;
            let requests = Arc::new(AtomicUsize::new(0));
            let calls = requests.clone();
            let router = Router::new().route(
                "/models",
                get(
                    move |headers: HeaderMap, Query(query): Query<HashMap<String, String>>| {
                        let count = calls.fetch_add(1, Ordering::SeqCst);
                        async move {
                            assert_eq!(
                                query.get("client_version").map(String::as_str),
                                Some("0.162.1")
                            );
                            assert_eq!(headers["ChatGPT-Account-ID"], "synthetic-account");
                            assert!(headers.contains_key("authorization"));
                            if count == 0 {
                                (
                                    StatusCode::SERVICE_UNAVAILABLE,
                                    Json(json!({"error":"private-provider-error"})),
                                )
                            } else {
                                (StatusCode::OK, Json(models()))
                            }
                        }
                    },
                ),
            );
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            Arc::get_mut(&mut service).unwrap().model_origin =
                format!("http://{}", listener.local_addr().unwrap());
            let server = spawn(async move {
                serve(listener, router).await.unwrap();
            });
            let initial = service.subscription_connection(&owner).await.unwrap();
            let error = service.subscription_catalog(&owner).await.err().unwrap();
            assert!(!error.message.contains("private-provider-error"));
            assert!(
                service
                    .subscription_connection(&owner)
                    .await
                    .unwrap()
                    .connected
            );
            assert!(service.models.list().await.unwrap().models.is_empty());
            let old = service.subscription_catalog(&owner).await.unwrap();
            let catalog = service.subscription_catalog(&owner).await.unwrap();
            assert_ne!(old.revision, catalog.revision);
            assert_eq!(catalog.models[0].name, "Account Model");
            assert!(!to_string(&catalog)
                .unwrap()
                .contains("private-catalog-prompt"));
            assert!(service
                .select_subscription(&owner, selection(&old))
                .await
                .is_err());
            assert!(service.models.list().await.unwrap().models.is_empty());
            let applying = service
                .select_subscription(&owner, selection(&catalog))
                .await
                .unwrap();
            assert!(matches!(applying.phase, SubscriptionPhase::Applying));
            loop {
                if matches!(
                    service.subscription_connection(&owner).await.unwrap().phase,
                    SubscriptionPhase::ApplyFailed
                ) {
                    break;
                }
                yield_now().await;
            }
            assert_eq!(service.models.list().await.unwrap().models.len(), 1);
            let retried = service
                .retry_subscription_apply(
                    &owner,
                    SubscriptionApplyRequest {
                        generation: catalog.generation.clone(),
                    },
                )
                .await
                .unwrap();
            assert!(matches!(retried.phase, SubscriptionPhase::Applying));
            loop {
                if matches!(
                    service.subscription_connection(&owner).await.unwrap().phase,
                    SubscriptionPhase::ApplyFailed
                ) {
                    break;
                }
                yield_now().await;
            }
            let restarted = OpenAiLoginService::new(
                service.runtime.clone(),
                service.auth.clone(),
                service.models.clone(),
            );
            let resumed = restarted.subscription_connection(&owner).await.unwrap();
            assert!(resumed.connected);
            assert_ne!(initial.generation, resumed.generation);
            assert!(restarted
                .select_subscription(&owner, selection(&catalog))
                .await
                .is_err());
            // A different account invalidates the old generation and snapshot even
            // when external credential installation uses the same shared handle.
            install(&service, "another-synthetic-account").await;
            assert!(service
                .select_subscription(&owner, selection(&catalog))
                .await
                .is_err());
            assert_eq!(service.models.list().await.unwrap().models.len(), 1);
            server.abort();
        })
        .await
        .expect("bounded catalog lifecycle");
    }

    #[tokio::test]
    async fn revocation_during_catalog_fetch_prevents_snapshot_and_selection() {
        timeout(Duration::from_secs(8), async {
            let (_dir, mut service, owner) = fixture().await;
            install(&service, "synthetic-account").await;
            let entered = Arc::new(Notify::new());
            let release = Arc::new(Notify::new());
            let start = entered.clone();
            let finish = release.clone();
            let router = Router::new().route(
                "/models",
                get(move || {
                    let start = start.clone();
                    let finish = finish.clone();
                    async move {
                        start.notify_one();
                        finish.notified().await;
                        Json(models())
                    }
                }),
            );
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            Arc::get_mut(&mut service).unwrap().model_origin =
                format!("http://{}", listener.local_addr().unwrap());
            let server = spawn(async move {
                serve(listener, router).await.unwrap();
            });
            let fetching = service.clone();
            let caller = owner.clone();
            let worker = spawn(async move { fetching.subscription_catalog(&caller).await });
            entered.notified().await;
            service
                .auth
                .revoke_credential(&owner, owner.credential_id())
                .await
                .unwrap();
            release.notify_one();
            assert!(worker.await.unwrap().is_err());
            assert!(service.connection.lock().await.snapshot.is_none());
            assert!(service.subscription_connection(&owner).await.is_err());
            assert!(service.models.list().await.unwrap().models.is_empty());
            server.abort();
        })
        .await
        .expect("bounded revoked catalog principal");
    }
    #[tokio::test]
    async fn removing_saved_credentials_invalidates_connection_and_catalog() {
        use std::fs::remove_file;
        let (_dir, service, owner) = fixture().await;
        install(&service, "synthetic-account").await;
        let initial = service.subscription_connection(&owner).await.unwrap();
        assert!(initial.connected);
        remove_file(service.runtime.subscription.path()).unwrap();
        let missing = service.subscription_connection(&owner).await.unwrap();
        assert!(!missing.connected);
        assert!(missing.generation.is_none());
        assert!(service.connection.lock().await.snapshot.is_none());
        install(&service, "synthetic-account").await;
        assert_ne!(
            initial.generation,
            service
                .subscription_connection(&owner)
                .await
                .unwrap()
                .generation
        );
    }
    #[tokio::test]
    async fn selection_commit_survives_request_end_and_status_stays_responsive() {
        use tokio::sync::Mutex;

        use crate::{api::models::SubscriptionCatalogModel, service::panel_models::ModelService};
        timeout(Duration::from_secs(8), async {
            let (_dir, mut service, owner) = fixture().await;
            install(&service, "synthetic-account").await;
            let gate = Arc::new(Mutex::new(()));
            let models = ModelService::new(
                service.runtime.path.clone(),
                service.runtime.base.clone(),
                gate.clone(),
            );
            Arc::get_mut(&mut service).unwrap().models = models;
            let connection = service.subscription_connection(&owner).await.unwrap();
            let catalog = SubscriptionCatalogView {
                generation: connection.generation.unwrap(),
                revision: "snapshot".into(),
                models: vec![SubscriptionCatalogModel {
                    slug: "account-model".into(),
                    name: "Account Model".into(),
                    description: None,
                    selectable: true,
                    unavailable_reason: None,
                    input_modalities: vec!["text".into()],
                    context_window: None,
                    reasoning_efforts: vec![],
                    default_reasoning_effort: None,
                }],
            };
            service.connection.lock().await.snapshot = Some(catalog.clone());
            let lease = gate.lock().await;
            let request = service
                .select_subscription(&owner, selection(&catalog))
                .await
                .unwrap();
            assert!(matches!(request.phase, SubscriptionPhase::Applying));
            drop(request);
            let status = timeout(
                Duration::from_millis(300),
                service.subscription_connection(&owner),
            )
            .await
            .unwrap()
            .unwrap();
            assert!(matches!(status.phase, SubscriptionPhase::Applying));
            drop(lease);
            loop {
                if matches!(
                    service.subscription_connection(&owner).await.unwrap().phase,
                    SubscriptionPhase::ApplyFailed
                ) {
                    break;
                }
                yield_now().await;
            }
            assert_eq!(service.models.list().await.unwrap().models.len(), 1);
        })
        .await
        .expect("detached model selection is bounded and recoverable");
    }
}
