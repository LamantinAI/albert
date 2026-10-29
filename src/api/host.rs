use std::{convert::Infallible, pin::Pin};

use async_trait::async_trait;
use futures::Stream;
use serde::Serialize;

use super::{
    errors::ApiResult,
    models::{
        ApplyRequest, ApplyView, ApprovalDecision, ApprovalList, ApprovalView, AttemptView,
        CancelRequest, ConfigDocument, ConfigList, ConfigWrite, EmptyObject, IdentityList,
        MessageCreate, MessageList, MessageReceipt, ModelOrder, ModelPoolView, ModelPreference,
        ModelWrite, PageQuery, ProfileView, RuntimeStatus, SecretList, SecretWrite, SessionCreate,
        SessionEvent, SessionList, SessionState, SessionUpdate, SessionView, SkillImport,
        SkillList, SkillUpdate, SkillView, SkillWrite, ValidationView,
    },
    principal::Principal,
};

pub type EventStream = Pin<Box<dyn Stream<Item = Result<SessionEvent, Infallible>> + Send>>;
pub struct SessionEventRequest {
    pub session_id: String,
    pub after: Option<String>,
    pub runtime_epoch: Option<String>,
}

/// Albert-owned policy/services implement this seam. The HTTP adapter never
/// reaches into cognition, the session journal or provider credentials directly.
#[async_trait]
pub trait Host: Send + Sync {
    async fn status(&self) -> ApiResult<RuntimeStatus>;
    async fn operation(
        &self,
        principal: &Principal,
        operation: HostOperation,
    ) -> ApiResult<HostResponse>;
    async fn events(
        &self,
        principal: Principal,
        request: SessionEventRequest,
    ) -> ApiResult<EventStream>;
}
pub enum HostOperation {
    Profile(ProfileOperation),
    Session(SessionOperation),
    Model(ModelOperation),
    Skill(SkillOperation),
    Config(ConfigOperation),
    Approval(ApprovalOperation),
}
pub enum ProfileOperation {
    Get,
    Identities,
}
pub enum SessionOperation {
    List(PageQuery),
    Create(SessionCreate),
    Get {
        id: String,
    },
    Update {
        id: String,
        input: SessionUpdate,
    },
    Messages {
        id: String,
        page: PageQuery,
    },
    Send {
        id: String,
        input: MessageCreate,
    },
    Receipt {
        id: String,
        request_id: String,
    },
    State {
        id: String,
    },
    Cancel {
        attempt_id: String,
        input: CancelRequest,
    },
}
pub enum ModelOperation {
    List,
    Create(ModelWrite),
    Update {
        id: String,
        input: ModelWrite,
    },
    Delete {
        id: String,
        revision: Option<String>,
    },
    Order(ModelOrder),
    Preference(ModelPreference),
}
pub enum SkillOperation {
    List,
    Get {
        id: String,
    },
    Put {
        id: String,
        input: SkillWrite,
    },
    Update {
        id: String,
        input: SkillUpdate,
    },
    Delete {
        id: String,
        revision: Option<String>,
    },
    Import(SkillImport),
}
pub enum ConfigOperation {
    List,
    Get { id: String },
    Put { id: String, input: ConfigWrite },
    Validate(ConfigWrite),
    Apply(ApplyRequest),
    Application { id: String },
    GetPrompt { id: String },
    PutPrompt { id: String, input: ConfigWrite },
    Secrets,
    PutSecret { id: String, input: SecretWrite },
    DeleteSecret { id: String },
}
pub enum ApprovalOperation {
    List,
    Decide { id: String, input: ApprovalDecision },
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum HostResponse {
    Profile(ProfileView),
    Identities(IdentityList),
    Session(SessionView),
    Sessions(SessionList),
    Messages(MessageList),
    Receipt(MessageReceipt),
    State(SessionState),
    Attempt(AttemptView),
    Models(ModelPoolView),
    Skill(SkillView),
    Skills(SkillList),
    Config(ConfigDocument),
    Configs(ConfigList),
    Validation(ValidationView),
    Apply(ApplyView),
    Secrets(SecretList),
    Approval(ApprovalView),
    Approvals(ApprovalList),
    Empty(EmptyObject),
}
