//! Owner-bound, bounded in-memory device authorization attempts.
mod catalog;
mod connection;
mod execution;

use std::{collections::VecDeque, sync::Arc};

use albert_core::{
    openai_login::{DeviceAuthEndpoints, DEVICE_AUTH_TTL},
    runtime::AgentRuntime,
};
use axum::http::StatusCode;
use chrono::{Duration as ChronoDuration, Utc};
use rand::random;
use tokio::{spawn, sync::Mutex, task::AbortHandle, time::Instant};

use crate::{
    api::errors::ApiError, api::errors::ApiResult, api::host::OpenAiLoginOperation,
    api::models::OpenAiLoginPhase, api::models::OpenAiLoginStart, api::models::OpenAiLoginView,
    api::principal::Principal, service::auth::AuthService, service::panel_models::ModelService,
};

const RETAIN: usize = 8;

pub(crate) struct OpenAiLoginService {
    runtime: Arc<AgentRuntime>,
    auth: Arc<AuthService>,
    models: ModelService,
    endpoints: Option<DeviceAuthEndpoints>,
    model_origin: String,
    attempts: Mutex<VecDeque<Attempt>>,
    credential_gate: Mutex<()>,
    connection: Mutex<connection::ConnectionState>,
}
struct Attempt {
    owner: String,
    credential: String,
    deadline: Instant,
    terminal: Option<Instant>,
    replace: bool,
    worker: Option<AbortHandle>,
    view: OpenAiLoginView,
}
impl Attempt {
    fn active(&self) -> bool {
        matches!(
            self.view.phase,
            OpenAiLoginPhase::Preparing | OpenAiLoginPhase::AwaitingDeviceCode
        )
    }
    fn expire(&mut self) {
        if self.active() && Instant::now() >= self.deadline {
            self.stop(
                OpenAiLoginPhase::Expired,
                "Authorization expired. Start a new attempt.",
            );
        }
    }
    fn finish(&mut self, phase: OpenAiLoginPhase, message: &str) {
        self.view.phase = phase;
        self.view.message = Some(message.into());
        self.view.verification_url = None;
        self.view.user_code = None;
        self.worker = None;
        self.terminal = Some(Instant::now());
    }
    fn stop(&mut self, phase: OpenAiLoginPhase, message: &str) {
        if let Some(worker) = self.worker.take() {
            worker.abort();
        }
        self.finish(phase, message);
    }
    fn belongs(&self, principal: &Principal) -> bool {
        self.owner == principal.account_id() && self.credential == principal.credential_id()
    }
}
impl OpenAiLoginService {
    pub fn new(
        runtime: Arc<AgentRuntime>,
        auth: Arc<AuthService>,
        models: ModelService,
    ) -> Arc<Self> {
        let model_origin = runtime.base.subscription_base_url.clone();
        #[cfg(feature = "local-e2e")]
        let model_origin = {
            use std::env::var;
            var("ALBERT_E2E_MODEL_ORIGIN").unwrap_or(model_origin)
        };
        Arc::new(Self {
            runtime,
            auth,
            models,
            endpoints: DeviceAuthEndpoints::startup().ok(),
            model_origin,
            attempts: Mutex::new(VecDeque::new()),
            credential_gate: Mutex::new(()),
            connection: Mutex::new(connection::ConnectionState::new()),
        })
    }

    pub async fn operation(
        self: &Arc<Self>,
        caller: &Principal,
        operation: OpenAiLoginOperation,
    ) -> ApiResult<OpenAiLoginView> {
        let caller = self.auth.validate_principal(caller).await?;
        caller.owner_permit()?;
        match operation {
            OpenAiLoginOperation::Start(input) => self.start(&caller, input).await,
            OpenAiLoginOperation::Get { id } => {
                let mut attempts = self.attempts.lock().await;
                Ok(Self::find(&mut attempts, &caller, &id)?.view.clone())
            }
            OpenAiLoginOperation::Cancel { id } => {
                let mut attempts = self.attempts.lock().await;
                let attempt = Self::find(&mut attempts, &caller, &id)?;
                if attempt.view.credentials_saved {
                    return Err(ApiError::conflict(
                        "Credentials are already connected. Continue to the model catalog.",
                    ));
                }
                if attempt.active() {
                    attempt.stop(OpenAiLoginPhase::Cancelled, "Authorization cancelled.");
                }
                Ok(attempt.view.clone())
            }
        }
    }
    fn find<'a>(
        attempts: &'a mut VecDeque<Attempt>,
        caller: &Principal,
        id: &str,
    ) -> ApiResult<&'a mut Attempt> {
        let attempt = attempts
            .iter_mut()
            .find(|attempt| attempt.view.id == id && attempt.belongs(caller))
            .ok_or_else(ApiError::not_found)?;
        attempt.expire();
        Ok(attempt)
    }
    async fn start(
        self: &Arc<Self>,
        caller: &Principal,
        input: OpenAiLoginStart,
    ) -> ApiResult<OpenAiLoginView> {
        let endpoints = self.endpoints.clone().ok_or_else(|| {
            ApiError::unavailable("Authorization is unavailable. Review the host settings.")
        })?;
        let mut attempts = self.attempts.lock().await;
        for attempt in attempts.iter_mut() {
            attempt.expire();
        }
        attempts.retain(|attempt| {
            attempt
                .terminal
                .is_none_or(|at| at.elapsed() < DEVICE_AUTH_TTL)
        });
        if let Some(existing) = attempts
            .iter()
            .find(|attempt| attempt.belongs(caller) && attempt.active())
        {
            return Ok(existing.view.clone());
        }
        if self.runtime.subscription.path().exists() && !input.replace_existing {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "subscription_replacement_required",
                "Subscription credentials already exist. Confirm replacement before reconnecting.",
            ));
        }
        if attempts.iter().any(Attempt::active) {
            return Err(ApiError::conflict(
                "An authorization attempt is already active. Complete or cancel it first.",
            ));
        }
        if self.connection.lock().await.applying() {
            return Err(ApiError::conflict(
                "Model settings are being applied. Wait before reconnecting.",
            ));
        }
        while attempts.len() >= RETAIN {
            attempts.pop_front();
        }
        let deadline = Instant::now() + DEVICE_AUTH_TTL;
        let view = OpenAiLoginView {
            id: format!("{:032x}", random::<u128>()),
            phase: OpenAiLoginPhase::Preparing,
            verification_url: None,
            user_code: None,
            expires_at: (Utc::now() + ChronoDuration::seconds(DEVICE_AUTH_TTL.as_secs() as i64))
                .to_rfc3339(),
            credentials_saved: false,
            message: None,
        };
        let service = Arc::clone(self);
        let principal = caller.clone();
        let id = view.id.clone();
        let worker = spawn(async move {
            service
                .authorize_device(principal, id, endpoints, deadline)
                .await;
        });
        attempts.push_back(Attempt {
            owner: caller.account_id().into(),
            credential: caller.credential_id().into(),
            deadline,
            terminal: None,
            replace: input.replace_existing,
            worker: Some(worker.abort_handle()),
            view: view.clone(),
        });
        Ok(view)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs::{read, write},
        sync::{atomic::AtomicBool, Arc},
        time::Duration,
    };

    use albert_core::{
        config::Config, openai_login::DeviceAuthEndpoints, runtime::AgentRuntime,
        service::session::SessionService, sessions::settings::SessionSettings,
        sessions::transport::LocalTransport,
    };
    use axum::{http::StatusCode, routing::post, serve, Json, Router};
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use octo_history::SqliteHistory;
    use serde_json::{json, to_string};
    use tempfile::{tempdir, TempDir};
    use tokio::{
        net::TcpListener,
        spawn,
        sync::{Mutex, Notify},
        task::JoinHandle,
        time::{sleep, timeout, Instant},
    };

    use super::OpenAiLoginService;
    use crate::{
        api::host::OpenAiLoginOperation, api::models::CredentialUpdate, api::models::LoginRequest,
        api::models::OpenAiLoginPhase, api::models::OpenAiLoginStart, api::models::OpenAiLoginView,
        api::principal::Principal, service::auth::AuthService, service::auth::AuthSettings,
        service::panel_models::ModelService,
    };

    pub(super) async fn fixture() -> (TempDir, Arc<OpenAiLoginService>, Principal) {
        let dir = tempdir().unwrap();
        let sessions = Arc::new(
            SessionService::from_history(
                Arc::new(
                    SqliteHistory::open_retained(dir.path().join("history.db"))
                        .await
                        .unwrap(),
                ),
                SessionSettings::default(),
            )
            .await
            .unwrap(),
        );
        let auth = AuthService::open(
            sessions.history().pool().clone(),
            dir.path().join("key"),
            AuthSettings::default(),
        )
        .await
        .unwrap();
        let login = auth
            .login(LoginRequest {
                username: "owner".into(),
                password: "owner".into(),
            })
            .await
            .unwrap();
        let owner = auth.authenticate(&login.token).await.unwrap();
        let (_, login) = auth
            .rotate_credentials(
                &owner,
                CredentialUpdate {
                    current_password: "owner".into(),
                    new_password: "test-rotated-password".into(),
                    new_username: None,
                },
            )
            .await
            .unwrap();
        let owner = auth.authenticate(&login.token).await.unwrap();
        let path = dir.path().join("albert.toml");
        write(&path, "[subscription]\nauth_json = 'state/auth.json'\n").unwrap();
        let config = Config::load_from(&path).unwrap();
        let models = ModelService::new(path.clone(), config.clone(), Arc::new(Mutex::new(())));
        let runtime = AgentRuntime::new(
            path,
            config.clone(),
            sessions,
            config.authorization.build(),
            Arc::new(AtomicBool::new(false)),
            Arc::new(LocalTransport),
        );
        // OAuth lifecycle fixtures represent an existing workspace with memory.
        runtime.initialize_memory_setup(false).await.unwrap();
        (dir, OpenAiLoginService::new(runtime, auth, models), owner)
    }
    fn start() -> OpenAiLoginOperation {
        OpenAiLoginOperation::Start(OpenAiLoginStart {
            replace_existing: false,
        })
    }

    async fn mock(
        service: &mut Arc<OpenAiLoginService>,
        exchange_gate: Option<(Arc<Notify>, Arc<Notify>)>,
    ) -> JoinHandle<()> {
        let payload = URL_SAFE_NO_PAD.encode(json!({"exp":9_999_999_999_i64,"https://api.openai.com/auth":{"chatgpt_account_id":"synthetic-account"}}).to_string());
        let tokens =
            json!({"access_token":format!("x.{payload}.x"),"refresh_token":"synthetic-refresh"});
        let can_authorize = exchange_gate.is_some();
        let router = Router::new()
            .route("/api/accounts/deviceauth/usercode", post(|| async {
                Json(json!({"device_auth_id":"private-device-id","user_code":"ABCD-EFGH","interval":"1"}))
            }))
            .route("/api/accounts/deviceauth/token", post(move || async move {
                use axum::response::IntoResponse;
                if !can_authorize {
                    return StatusCode::NOT_FOUND.into_response();
                }
                Json(json!({"authorization_code":"private-auth-code","code_verifier":"private-verifier"})).into_response()
            }))
            .route("/oauth/token", post(move || {
                let gate = exchange_gate.clone();
                let tokens = tokens.clone();
                async move {
                    if let Some((entered, release)) = gate {
                        entered.notify_one();
                        release.notified().await;
                    }
                    Json(tokens)
                }
            }));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        Arc::get_mut(service).unwrap().endpoints = Some(DeviceAuthEndpoints::test_origin(&origin));
        spawn(async move {
            serve(listener, router).await.unwrap();
        })
    }

    async fn status(
        service: &Arc<OpenAiLoginService>,
        owner: &Principal,
        id: &str,
    ) -> OpenAiLoginView {
        service
            .operation(owner, OpenAiLoginOperation::Get { id: id.into() })
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn attempts_are_single_writer_owner_bound_expiring_and_bounded() {
        let (_dir, mut service, owner) = fixture().await;
        let server = mock(&mut service, None).await;
        let first = service.operation(&owner, start()).await.unwrap();
        assert_eq!(
            service.operation(&owner, start()).await.unwrap().id,
            first.id
        );
        let mut attempts = service.attempts.lock().await;
        let foreign = Principal::authenticated(
            owner.account().clone(),
            "another-credential".into(),
            owner.csrf().into(),
        );
        assert!(OpenAiLoginService::find(&mut attempts, &foreign, &first.id).is_err());
        attempts[0].deadline = Instant::now();
        drop(attempts);
        let expired = status(&service, &owner, &first.id).await;
        assert!(matches!(expired.phase, OpenAiLoginPhase::Expired));
        assert!(expired.user_code.is_none());
        for _ in 0..12 {
            let view = service.operation(&owner, start()).await.unwrap();
            service
                .operation(&owner, OpenAiLoginOperation::Cancel { id: view.id })
                .await
                .unwrap();
        }
        assert!(service.attempts.lock().await.len() <= 8);
        server.abort();
    }

    #[tokio::test]
    async fn cancellation_and_revocation_fence_an_in_flight_provider_result() {
        timeout(Duration::from_secs(15), async {
            for revoke in [false, true] {
                let (_dir, mut service, owner) = fixture().await;
                let entered = Arc::new(Notify::new());
                let release = Arc::new(Notify::new());
                let server = mock(&mut service, Some((entered.clone(), release.clone()))).await;
                let view = service.operation(&owner, start()).await.unwrap();
                entered.notified().await;
                let ready = status(&service, &owner, &view.id).await;
                assert_eq!(ready.user_code.as_deref(), Some("ABCD-EFGH"));
                assert!(!to_string(&ready).unwrap().contains("private-"));
                let worker = service.attempts.lock().await[0].worker.clone().unwrap();
                if revoke {
                    service
                        .auth
                        .revoke_credential(&owner, owner.credential_id())
                        .await
                        .unwrap();
                } else {
                    service
                        .operation(
                            &owner,
                            OpenAiLoginOperation::Cancel {
                                id: view.id.clone(),
                            },
                        )
                        .await
                        .unwrap();
                }
                release.notify_one();
                while !worker.is_finished() {
                    sleep(Duration::from_millis(10)).await;
                }
                assert!(!service.runtime.subscription.path().exists());
                let attempts = service.attempts.lock().await;
                assert!(matches!(
                    attempts[0].view.phase,
                    OpenAiLoginPhase::Cancelled | OpenAiLoginPhase::Failed
                ));
                assert!(attempts[0].view.user_code.is_none());
                server.abort();
            }
        })
        .await
        .expect("bounded provider race tests");
    }

    #[tokio::test]
    async fn device_completion_saves_only_credentials_and_survives_service_restart() {
        timeout(Duration::from_secs(15), async {
            let (_dir, mut service, owner) = fixture().await;
            let entered = Arc::new(Notify::new());
            let release = Arc::new(Notify::new());
            let server = mock(&mut service, Some((entered.clone(), release.clone()))).await;
            let before = read(&service.runtime.path).unwrap();
            let started = service.operation(&owner, start()).await.unwrap();
            entered.notified().await;
            release.notify_one();
            loop {
                let result = status(&service, &owner, &started.id).await;
                if !matches!(result.phase, OpenAiLoginPhase::AwaitingDeviceCode) {
                    assert!(matches!(result.phase, OpenAiLoginPhase::Connected));
                    assert!(result.credentials_saved);
                    assert!(result.user_code.is_none());
                    assert!(!to_string(&result).unwrap().contains("synthetic-refresh"));
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
            assert_eq!(read(&service.runtime.path).unwrap(), before);
            assert!(service.models.list().await.unwrap().models.is_empty());
            let connected = service.subscription_connection(&owner).await.unwrap();
            let restarted = OpenAiLoginService::new(
                service.runtime.clone(),
                service.auth.clone(),
                service.models.clone(),
            );
            let resumed = restarted.subscription_connection(&owner).await.unwrap();
            assert!(resumed.connected);
            assert_ne!(connected.generation, resumed.generation);
            assert!(restarted.models.list().await.unwrap().models.is_empty());
            server.abort();
        })
        .await
        .expect("bounded auth completion");
    }
}
