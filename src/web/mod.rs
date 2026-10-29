mod approvals;
mod assets;
mod profile;
pub(crate) mod runtime;
pub(crate) mod sessions;
mod startup;

pub(crate) use profile::configured_owner;
pub(crate) use startup::run;

use std::sync::Arc;

use async_trait::async_trait;
use axum::Router;
use octo_connector_web::WebApplication;
use octo_core::{ConnectorContext, Envelope, OctoResult};
use tokio::sync::Mutex;

use self::{approvals::ApprovalService, runtime::AgentRuntime, sessions::SessionHost};
use crate::{
    api::{
        errors::ApiResult,
        host::{
            EventStream, Host, HostOperation, HostResponse, ModelOperation, ProfileOperation,
            SessionEventRequest, SkillOperation,
        },
        models::RuntimeStatus,
        principal::Principal,
        router_with_body_limit as api_router,
        state::ApiState,
    },
    service::{
        auth::AuthService, control::ControlService, panel_models::ModelService,
        panel_skills::SkillService,
    },
};

pub(crate) struct Panel {
    pub runtime: Arc<AgentRuntime>,
    pub auth: Arc<AuthService>,
    pub sessions: Arc<SessionHost>,
    models: ModelService,
    skills: SkillService,
    control: ControlService,
    approvals: ApprovalService,
}

impl Panel {
    pub fn new(runtime: Arc<AgentRuntime>, auth: Arc<AuthService>) -> Arc<Self> {
        let gate = Arc::new(Mutex::new(()));
        Arc::new(Self {
            sessions: SessionHost::new(runtime.clone(), auth.clone()),
            models: ModelService::new(runtime.path.clone(), runtime.base.clone(), gate.clone()),
            skills: SkillService::new(runtime.base.clone(), gate.clone()),
            control: ControlService::new(runtime.clone(), gate),
            approvals: ApprovalService::new(runtime.clone()),
            runtime,
            auth,
        })
    }
}

#[async_trait]
impl Host for Panel {
    async fn status(&self) -> ApiResult<RuntimeStatus> {
        Ok(self.runtime.status())
    }
    async fn operation(
        &self,
        caller: &Principal,
        operation: HostOperation,
    ) -> ApiResult<HostResponse> {
        match operation {
            HostOperation::Profile(ProfileOperation::Get) => {
                Ok(HostResponse::Profile(self.profile(caller).await?))
            }
            HostOperation::Profile(ProfileOperation::Identities) => {
                Ok(HostResponse::Identities(self.identities(caller).await?))
            }
            HostOperation::Session(operation) => self.sessions.operation(caller, operation).await,
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
                        let _ = agent.panel_prefer(&id);
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

pub(crate) struct HostedPanel(pub Arc<Panel>, pub usize);
#[async_trait]
impl WebApplication for HostedPanel {
    async fn router(&self, context: ConnectorContext) -> OctoResult<Router> {
        self.0.sessions.attach(context.clone());
        self.0.approvals.attach(context);
        Ok(
            api_router(ApiState::new(self.0.auth.clone(), self.0.clone()), self.1)
                .merge(assets::router()),
        )
    }
    async fn on_event(&self, _context: &ConnectorContext, event: Envelope) -> OctoResult<()> {
        self.0.sessions.observe(&event);
        self.0.approvals.observe(&event).await;
        Ok(())
    }
}
