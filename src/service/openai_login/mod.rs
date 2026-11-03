//! Owner-bound, bounded in-memory authorization attempts. No callback listener or
//! durable OAuth secrets: restart requires a new attempt.
mod catalog;
mod connection;
mod execution;

use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant},
};

use axum::http::StatusCode;
use chrono::{Duration as ChronoDuration, Utc};
use rand::random;
use tokio::{spawn, sync::Mutex};

use crate::{
    api::{
        errors::{ApiError, ApiResult},
        host::OpenAiLoginOperation,
        models::{OpenAiLoginPhase, OpenAiLoginStart, OpenAiLoginSubmission, OpenAiLoginView},
        principal::Principal,
    },
    openai_login::{LoginFlow, OAuthEndpoints, REDIRECT_URI},
    service::{auth::AuthService, panel_models::ModelService},
    web::runtime::AgentRuntime,
};

const TTL: Duration = Duration::from_secs(600);
const RETAIN: usize = 8;

pub(crate) struct OpenAiLoginService {
    runtime: Arc<AgentRuntime>,
    auth: Arc<AuthService>,
    models: ModelService,
    endpoints: Option<OAuthEndpoints>,
    model_origin: String,
    attempts: Mutex<VecDeque<Attempt>>,
    credential_gate: Mutex<()>,
    connection: Mutex<connection::ConnectionState>,
}
struct Attempt {
    owner: String,
    credential: String,
    created: Instant,
    terminal: Option<Instant>,
    replace: bool,
    flow: Option<LoginFlow>,
    view: OpenAiLoginView,
}
impl Attempt {
    fn active(&self) -> bool {
        matches!(
            self.view.phase,
            OpenAiLoginPhase::AwaitingCallback | OpenAiLoginPhase::Exchanging
        )
    }
    fn expire(&mut self) {
        if self.active() && self.created.elapsed() >= TTL && !self.view.credentials_saved {
            self.finish(
                OpenAiLoginPhase::Expired,
                "Authorization expired. Start a new attempt.",
            );
        }
    }
    fn finish(&mut self, phase: OpenAiLoginPhase, message: &str) {
        self.view.phase = phase;
        self.view.message = Some(message.into());
        self.view.authorize_url = None;
        self.flow = None;
        self.terminal = Some(Instant::now());
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
            endpoints: OAuthEndpoints::startup().ok(),
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
            OpenAiLoginOperation::Submit { id, input } => self.submit(caller, id, input).await,
            OpenAiLoginOperation::Cancel { id } => {
                let mut attempts = self.attempts.lock().await;
                let attempt = Self::find(&mut attempts, &caller, &id)?;
                if attempt.view.credentials_saved {
                    return Err(ApiError::conflict(
                        "Credentials are already connected. Continue to the model catalog.",
                    ));
                }
                if attempt.active() {
                    attempt.finish(OpenAiLoginPhase::Cancelled, "Authorization cancelled.");
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
        &self,
        caller: &Principal,
        input: OpenAiLoginStart,
    ) -> ApiResult<OpenAiLoginView> {
        let endpoints = self.endpoints.as_ref().ok_or_else(|| {
            ApiError::unavailable("Authorization is unavailable. Review the host settings.")
        })?;
        let mut attempts = self.attempts.lock().await;
        for attempt in attempts.iter_mut() {
            attempt.expire();
        }
        attempts.retain(|attempt| attempt.terminal.is_none_or(|at| at.elapsed() < TTL));
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
        let flow = LoginFlow::new(endpoints);
        let view = OpenAiLoginView {
            id: format!("{:032x}", random::<u128>()),
            phase: OpenAiLoginPhase::AwaitingCallback,
            authorize_url: Some(flow.authorize_url().into()),
            redirect_uri: REDIRECT_URI.into(),
            expires_at: (Utc::now() + ChronoDuration::seconds(TTL.as_secs() as i64)).to_rfc3339(),
            credentials_saved: false,
            message: None,
        };
        attempts.push_back(Attempt {
            owner: caller.account_id().into(),
            credential: caller.credential_id().into(),
            created: Instant::now(),
            terminal: None,
            replace: input.replace_existing,
            flow: Some(flow),
            view: view.clone(),
        });
        Ok(view)
    }
    async fn submit(
        self: &Arc<Self>,
        caller: Principal,
        id: String,
        input: OpenAiLoginSubmission,
    ) -> ApiResult<OpenAiLoginView> {
        let mut attempts = self.attempts.lock().await;
        let attempt = Self::find(&mut attempts, &caller, &id)?;
        if !matches!(attempt.view.phase, OpenAiLoginPhase::AwaitingCallback) {
            return Err(ApiError::conflict(
                "This authorization attempt no longer accepts a callback.",
            ));
        }
        let code = attempt.flow.as_ref().ok_or_else(ApiError::internal)?.parse_submission(&input.callback)
            .map_err(|_| ApiError::invalid("The authorization result is invalid, declined, or belongs to another attempt. Check the paste or cancel and retry."))?
            .ok_or_else(|| ApiError::invalid("Paste the complete callback URL or authorization code."))?;
        let flow = attempt.flow.take().ok_or_else(ApiError::internal)?;
        attempt.view.phase = OpenAiLoginPhase::Exchanging;
        attempt.view.authorize_url = None;
        let result = attempt.view.clone();
        let service = Arc::clone(self);
        spawn(async move {
            service.exchange_and_install(caller, id, flow, code).await;
        });
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs::{read, write},
        sync::{atomic::AtomicBool, Arc},
        time::Instant,
    };

    use axum::{routing::post, serve, Json, Router};
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use octo_history::SqliteHistory;
    use serde_json::{json, to_string};
    use tempfile::{tempdir, TempDir};
    use tokio::{
        net::TcpListener,
        spawn,
        sync::{Mutex, Notify},
        task::yield_now,
        time::timeout,
    };

    use super::{OpenAiLoginService, TTL};
    use crate::{
        api::{
            host::OpenAiLoginOperation,
            models::{
                CredentialUpdate, LoginRequest, OpenAiLoginPhase, OpenAiLoginStart,
                OpenAiLoginSubmission,
            },
            principal::Principal,
        },
        config::Config,
        openai_login::OAuthEndpoints,
        service::{
            auth::{AuthService, AuthSettings},
            panel_models::ModelService,
            session::SessionService,
        },
        sessions::settings::SessionSettings,
        web::runtime::AgentRuntime,
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
        );
        (dir, OpenAiLoginService::new(runtime, auth, models), owner)
    }
    fn start() -> OpenAiLoginOperation {
        OpenAiLoginOperation::Start(OpenAiLoginStart {
            replace_existing: false,
        })
    }

    #[tokio::test]
    async fn attempts_are_single_writer_owner_bound_expiring_and_bounded() {
        let (_dir, service, owner) = fixture().await;
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
        attempts[0].created = Instant::now() - TTL;
        drop(attempts);
        assert!(matches!(
            service
                .operation(&owner, OpenAiLoginOperation::Get { id: first.id })
                .await
                .unwrap()
                .phase,
            OpenAiLoginPhase::Expired
        ));
        for _ in 0..12 {
            let view = service.operation(&owner, start()).await.unwrap();
            service
                .operation(&owner, OpenAiLoginOperation::Cancel { id: view.id })
                .await
                .unwrap();
        }
        assert!(service.attempts.lock().await.len() <= 8);
    }

    #[tokio::test]
    async fn cancellation_and_revocation_fence_an_in_flight_provider_result() {
        timeout(TTL / 60, async {
            for revoke in [false, true] {
                let (_dir, mut service, owner) = fixture().await;
                let entered = Arc::new(Notify::new()); let release = Arc::new(Notify::new());
                let entry = entered.clone(); let exit = release.clone();
                let payload = URL_SAFE_NO_PAD.encode(json!({"exp":9_999_999_999_i64,"https://api.openai.com/auth":{"chatgpt_account_id":"synthetic-account"}}).to_string());
                let body = json!({"access_token":format!("x.{payload}.x"),"refresh_token":"synthetic-refresh"});
                let router = Router::new().route("/token", post(move || { let entry=entry.clone(); let exit=exit.clone(); let body=body.clone(); async move { entry.notify_one(); exit.notified().await; Json(body) } }));
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let url = format!("http://{}/token", listener.local_addr().unwrap());
                let server = spawn(async move { serve(listener, router).await.unwrap(); });
                Arc::get_mut(&mut service).unwrap().endpoints = Some(OAuthEndpoints::test_endpoint(url));
                let view = service.operation(&owner, start()).await.unwrap();
                let flow = {
                    let mut attempts = service.attempts.lock().await;
                    attempts[0].view.phase = OpenAiLoginPhase::Exchanging;
                    attempts[0].flow.take().unwrap()
                };
                let worker_service = service.clone(); let worker_owner = owner.clone(); let id = view.id.clone();
                let worker = spawn(async move { worker_service.exchange_and_install(worker_owner, id, flow, "synthetic-code".into()).await; });
                entered.notified().await;
                if revoke {
                    service.auth.revoke_credential(&owner, owner.credential_id()).await.unwrap();
                } else {
                    service.operation(&owner, OpenAiLoginOperation::Cancel { id: view.id.clone() }).await.unwrap();
                    service.operation(&owner, start()).await.unwrap();
                }
                release.notify_one(); worker.await.unwrap(); server.abort();
                assert!(!service.runtime.subscription.path().exists());
                let attempts = service.attempts.lock().await;
                assert!(matches!(attempts[0].view.phase, OpenAiLoginPhase::Cancelled | OpenAiLoginPhase::Failed));
            }
        }).await.expect("bounded provider race tests");
    }
    #[tokio::test]
    async fn oauth_completion_saves_only_credentials_and_survives_attempt_expiry_and_service_restart(
    ) {
        timeout(TTL / 60, async {
            let (_dir, mut service, owner) = fixture().await;
            let payload = URL_SAFE_NO_PAD.encode(json!({"exp":9_999_999_999_i64,"https://api.openai.com/auth":{"chatgpt_account_id":"synthetic-account"}}).to_string());
            let body = json!({"access_token":format!("x.{payload}.x"),"refresh_token":"synthetic-refresh"});
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/token", listener.local_addr().unwrap());
            let router = Router::new().route("/token", post(move || { let body=body.clone(); async move { Json(body) } }));
            let server = spawn(async move { serve(listener, router).await.unwrap(); });
            Arc::get_mut(&mut service).unwrap().endpoints = Some(OAuthEndpoints::test_endpoint(url));
            let before = read(&service.runtime.path).unwrap();
            let started = service.operation(&owner, start()).await.unwrap();
            service.operation(&owner, OpenAiLoginOperation::Submit { id: started.id.clone(), input: OpenAiLoginSubmission { callback: "synthetic-code".into() } }).await.unwrap();
            loop {
                let result = service.operation(&owner, OpenAiLoginOperation::Get { id: started.id.clone() }).await.unwrap();
                if matches!(result.phase, OpenAiLoginPhase::Connected) {
                    assert!(result.credentials_saved); assert!(!to_string(&result).unwrap().contains("synthetic-refresh")); break;
                }
                yield_now().await;
            }
            assert_eq!(read(&service.runtime.path).unwrap(), before);
            assert!(service.models.list().await.unwrap().models.is_empty());
            let connected = service.subscription_connection(&owner).await.unwrap();
            service.attempts.lock().await[0].created = Instant::now() - TTL;
            assert!(service.subscription_connection(&owner).await.unwrap().connected);
            let restarted = OpenAiLoginService::new(service.runtime.clone(), service.auth.clone(), service.models.clone());
            let resumed = restarted.subscription_connection(&owner).await.unwrap();
            assert!(resumed.connected); assert_ne!(connected.generation, resumed.generation);
            assert!(restarted.models.list().await.unwrap().models.is_empty());
            server.abort();
        }).await.expect("bounded auth completion");
    }
}
