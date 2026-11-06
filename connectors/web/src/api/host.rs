use std::{convert::Infallible, pin::Pin};

use async_trait::async_trait;
use futures::Stream;
use serde::Serialize;

use super::models::{
    ConnectorList, TelegramConnectionList, TelegramConnectionView, TelegramCredentialWrite,
    TelegramLoginTokenView,
};

use super::{
    errors::ApiError, errors::ApiResult, media::MediaOperation, media::MediaResponse,
    models::ApplyRequest, models::ApplyView, models::ApprovalDecision, models::ApprovalList,
    models::ApprovalView, models::AttemptView, models::CancelRequest, models::ConfigDocument,
    models::ConfigDraftInput, models::ConfigDraftView, models::ConfigList, models::ConfigWrite,
    models::EmptyObject, models::IdentityList, models::MessageCreate, models::MessageList,
    models::MessageReceipt, models::ModelOrder, models::ModelPoolView, models::ModelPreference,
    models::ModelWrite, models::OpenAiLoginStart, models::OpenAiLoginSubmission,
    models::OpenAiLoginView, models::PageQuery, models::ProfileView, models::RuntimeStatus,
    models::SecretList, models::SecretWrite, models::SessionCreate, models::SessionEvent,
    models::SessionList, models::SessionStart, models::SessionStartView, models::SessionState,
    models::SessionUpdate, models::SessionView, models::SkillImport, models::SkillList,
    models::SkillUpdate, models::SkillView, models::SkillWrite, models::SubscriptionApplyRequest,
    models::SubscriptionCatalogView, models::SubscriptionConnectionView,
    models::SubscriptionSelection, models::ValidationView, principal::Principal,
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
    async fn media(
        &self,
        _principal: &Principal,
        _operation: MediaOperation,
    ) -> ApiResult<MediaResponse> {
        Err(ApiError::not_found())
    }
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
    Connectors,
    Telegram(TelegramOperation),
    OpenAiLogin(OpenAiLoginOperation),
    OpenAiSubscription(SubscriptionOperation),
    Profile(ProfileOperation),
    Session(SessionOperation),
    Model(ModelOperation),
    Skill(SkillOperation),
    Config(ConfigOperation),
    Approval(ApprovalOperation),
}
pub enum OpenAiLoginOperation {
    Start(OpenAiLoginStart),
    Get {
        id: String,
    },
    Submit {
        id: String,
        input: OpenAiLoginSubmission,
    },
    Cancel {
        id: String,
    },
}
pub enum SubscriptionOperation {
    Connection,
    Catalog,
    Select(SubscriptionSelection),
    Apply(SubscriptionApplyRequest),
}
pub enum ProfileOperation {
    Get,
    Identities,
}
pub enum SessionOperation {
    Composer,
    Compact {
        id: String,
        input: super::models::CompactCreate,
    },
    Start(SessionStart),
    StartGet {
        id: String,
    },
    StartCancel {
        id: String,
    },
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
    Draft { id: String, input: ConfigDraftInput },
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
    Composer(super::models::ComposerView),
    Connectors(ConnectorList),
    TelegramConnections(TelegramConnectionList),
    TelegramConnection(TelegramConnectionView),
    TelegramToken(TelegramLoginTokenView),
    OpenAiLogin(OpenAiLoginView),
    SubscriptionConnection(SubscriptionConnectionView),
    SubscriptionCatalog(SubscriptionCatalogView),
    Profile(ProfileView),
    Identities(IdentityList),
    Session(SessionView),
    Start(SessionStartView),
    Sessions(SessionList),
    Messages(MessageList),
    Receipt(MessageReceipt),
    State(SessionState),
    Attempt(AttemptView),
    Models(ModelPoolView),
    Skill(SkillView),
    Skills(SkillList),
    Config(ConfigDocument),
    ConfigDraft(ConfigDraftView),
    Configs(ConfigList),
    Validation(ValidationView),
    Apply(ApplyView),
    Secrets(SecretList),
    Approval(ApprovalView),
    Approvals(ApprovalList),
    Empty(EmptyObject),
}

pub enum TelegramOperation {
    List,
    Status {
        id: String,
    },
    Credential {
        id: String,
        input: TelegramCredentialWrite,
    },
    Issue {
        id: String,
    },
    RevokeToken {
        id: String,
    },
    Disconnect {
        id: String,
    },
}
