use crate::api::models::OperatorQuery;
use crate::service::{mcp::McpService, memory::MemoryService};
mod approvals;
#[cfg(feature = "embedded-panel")]
mod assets;
mod mount;
mod profile;
mod resources;
pub(crate) mod sessions;
mod telegram;
#[cfg(test)]
pub(crate) mod testing;

pub use profile::configured_owner;

use std::sync::Arc;

use albert_core::runtime::AgentRuntime;
use async_trait::async_trait;
use axum::Router;
use octo_connector_web::WebApplication;
use octo_core::{ConnectorContext, Envelope, OctoResult};
use tokio::sync::Mutex;

use self::{approvals::ApprovalService, sessions::SessionHost};
use crate::{
    api::errors::ApiError, api::errors::ApiResult, api::host::EventStream, api::host::Host,
    api::host::HostOperation, api::host::HostResponse, api::host::ModelOperation,
    api::host::ProfileOperation, api::host::SessionEventRequest, api::host::SkillOperation,
    api::host::SubscriptionOperation, api::media::MediaOperation, api::media::MediaResponse,
    api::models::RuntimeStatus, api::principal::Principal,
    api::router_with_media_limit as api_router, api::state::ApiState, service::auth::AuthService,
    service::control::ControlService, service::openai_login::OpenAiLoginService,
    service::panel_models::ModelService, service::panel_skills::SkillService,
};

pub struct Panel {
    pub runtime: Arc<AgentRuntime>,
    pub auth: Arc<AuthService>,
    pub(crate) sessions: Arc<SessionHost>,
    models: ModelService,
    mcp: McpService,
    memory: MemoryService,
    openai_login: Arc<OpenAiLoginService>,
    skills: SkillService,
    control: ControlService,
    approvals: Arc<ApprovalService>,
}

impl Panel {
    pub fn new(runtime: Arc<AgentRuntime>, auth: Arc<AuthService>) -> Arc<Self> {
        auth.attach_accounts(runtime.sessions.accounts().clone());
        runtime.sessions.credentials().enable_ui();
        runtime.sessions.confirmations().enable_ui();
        let gate = Arc::new(Mutex::new(()));
        let models = ModelService::new(runtime.path.clone(), runtime.base.clone(), gate.clone());
        let openai_login = OpenAiLoginService::new(runtime.clone(), auth.clone(), models.clone());
        let approvals = Arc::new(ApprovalService::new(runtime.clone()));
        let sessions = SessionHost::new(runtime.clone(), auth.clone());
        sessions.bind_approvals(approvals.clone());
        Arc::new(Self {
            sessions,
            models,
            mcp: McpService::new(runtime.clone(), gate.clone()),
            memory: MemoryService::new(runtime.clone(), gate.clone()),
            openai_login,
            skills: SkillService::new(runtime.base.clone(), gate.clone()),
            control: ControlService::new(runtime.clone(), gate),
            approvals,
            runtime,
            auth,
        })
    }
}

#[async_trait]
impl Host for Panel {
    async fn operator_events(
        &self,
        caller: Principal,
        query: OperatorQuery,
    ) -> ApiResult<EventStream> {
        self.sessions.operator_stream(caller, query).await
    }
    async fn media(
        &self,
        caller: &Principal,
        operation: MediaOperation,
    ) -> ApiResult<MediaResponse> {
        self.sessions.media(caller, operation).await
    }
    async fn status(&self) -> ApiResult<RuntimeStatus> {
        Ok(self.runtime.status().into())
    }
    async fn operation(
        &self,
        caller: &Principal,
        operation: HostOperation,
    ) -> ApiResult<HostResponse> {
        match operation {
            HostOperation::Operator(query) => Ok(HostResponse::Operator(
                self.sessions.operator_snapshot(caller, &query).await?,
            )),
            HostOperation::ModelUsage => self.resource_usage(caller).await,
            HostOperation::SubscriptionLimits => self.resource_limits(caller).await,
            HostOperation::Credential(operation) => {
                self.sessions.credential_operation(caller, operation).await
            }
            HostOperation::Connectors => Ok(HostResponse::Connectors(
                self.control.connector_catalog(caller)?,
            )),
            HostOperation::Telegram(operation) => self.telegram_operation(caller, operation).await,
            HostOperation::OpenAiSubscription(operation) => match operation {
                SubscriptionOperation::Connection => Ok(HostResponse::SubscriptionConnection(
                    self.openai_login.subscription_connection(caller).await?,
                )),
                SubscriptionOperation::Catalog => Ok(HostResponse::SubscriptionCatalog(
                    self.openai_login.subscription_catalog(caller).await?,
                )),
                SubscriptionOperation::Select(input) => Ok(HostResponse::SubscriptionConnection(
                    self.openai_login.select_subscription(caller, input).await?,
                )),
                SubscriptionOperation::Apply(input) => Ok(HostResponse::SubscriptionConnection(
                    self.openai_login
                        .retry_subscription_apply(caller, input)
                        .await?,
                )),
            },
            HostOperation::OpenAiLogin(operation) => Ok(HostResponse::OpenAiLogin(
                self.openai_login.operation(caller, operation).await?,
            )),
            HostOperation::Profile(ProfileOperation::Get) => {
                Ok(HostResponse::Profile(self.profile(caller).await?))
            }
            HostOperation::Profile(ProfileOperation::Identities) => {
                Ok(HostResponse::Identities(self.identities(caller).await?))
            }
            HostOperation::Session(operation) => self.sessions.operation(caller, operation).await,
            HostOperation::Mcp(operation) => self.mcp.operation(caller, operation).await,
            HostOperation::Memory(operation) => self.memory.operation(caller, operation).await,
            HostOperation::Model(operation) => {
                let permit = caller.owner_permit()?;
                let changed = !matches!(&operation, ModelOperation::List);
                let preferred = match &operation {
                    ModelOperation::Preference(input) => Some(input.model_id.clone()),
                    _ => None,
                };
                let mut result = self.models.operation(&permit, operation).await?;
                if changed {
                    let _ = self.runtime.activate().await;
                }
                if let Some(agent) = self.runtime.current() {
                    if let Some(id) = preferred {
                        agent.panel_prefer(&id).map_err(|_| ApiError::conflict("The operator is saved, but its runtime selection could not be applied. Reload the model pool."))?;
                    }
                    result.preferred = Some(agent.panel_models().selected);
                }
                Ok(HostResponse::Models(result))
            }
            HostOperation::Skill(operation) => {
                let permit = caller.owner_permit()?;
                let changed = !matches!(
                    &operation,
                    SkillOperation::List | SkillOperation::Get { .. }
                );
                let result = self.skills.operation(&permit, operation).await?;
                if changed {
                    let _ = self.runtime.activate().await;
                }
                Ok(result)
            }
            HostOperation::Config(operation) => {
                self.control
                    .operation(&caller.owner_permit()?, operation)
                    .await
            }
            HostOperation::Approval(operation) => self.approvals.operation(caller, operation).await,
        }
    }
    async fn events(
        &self,
        caller: Principal,
        request: SessionEventRequest,
    ) -> ApiResult<EventStream> {
        self.sessions.events(caller, request).await
    }
}

pub struct HostedPanel {
    pub panel: Arc<Panel>,
    pub max_body_bytes: usize,
}
#[async_trait]
impl WebApplication for HostedPanel {
    async fn router(&self, context: ConnectorContext) -> OctoResult<Router> {
        self.panel.sessions.attach(context.clone());
        self.panel.approvals.attach(context);
        let routes = api_router(
            ApiState::new(self.panel.auth.clone(), self.panel.clone()),
            self.max_body_bytes,
            self.panel.runtime.sessions.media().limits.max_file_bytes,
        );
        #[cfg(feature = "embedded-panel")]
        let routes = routes.merge(assets::router());
        Ok(mount::router(routes))
    }

    async fn on_event(&self, _context: &ConnectorContext, event: Envelope) -> OctoResult<()> {
        self.panel.sessions.deliver_file(_context, &event).await;
        self.panel.sessions.observe(&event);
        self.panel.approvals.observe(&event).await;
        Ok(())
    }
}
