use async_trait::async_trait;
use serde::Serialize;

use super::models::{ModelUsageSummary, OperatorQuery, OperatorSnapshot, SubscriptionLimitsView};
use super::{
    mcp::McpOperation, memory::MemoryOperation, memory::MemoryView, models::McpServerList,
    models::McpToolList,
};

use super::models::{
    ConnectorList, CredentialCreate, CredentialRequestList, CredentialRequestView,
    CredentialSubmission, TelegramConnectionList, TelegramConnectionView, TelegramCredentialWrite,
    TelegramLoginTokenView,
};

use super::{
    errors::ApiError, errors::ApiResult, media::MediaOperation, media::MediaResponse,
    models::ApplyRequest, models::ApplyView, models::ApprovalDecision, models::ApprovalList,
    models::ApprovalListQuery, models::ApprovalView, models::AttemptView, models::CancelRequest,
    models::ConfigDocument, models::ConfigDraftInput, models::ConfigDraftView, models::ConfigList,
    models::ConfigWrite, models::EmptyObject, models::IdentityList, models::MessageCreate,
    models::MessageList, models::MessageReceipt, models::ModelOrder, models::ModelPoolView,
    models::ModelPreference, models::ModelRoleWrite, models::ModelWrite, models::OpenAiLoginStart,
    models::OpenAiLoginSubmission, models::OpenAiLoginView, models::PageQuery, models::ProfileView,
    models::RuntimeStatus, models::SecretList, models::SecretWrite, models::SessionCreate,
    models::SessionList, models::SessionStart, models::SessionStartView, models::SessionState,
    models::SessionUpdate, models::SessionView, models::SkillImport, models::SkillList,
    models::SkillUpdate, models::SkillView, models::SkillWrite, models::SubscriptionApplyRequest,
    models::SubscriptionCatalogView, models::SubscriptionConnectionView,
    models::SubscriptionSelection, models::ValidationView, principal::Principal,
};

mod events;
pub use events::{EventStream, SessionEventRequest};

/// Albert-owned policy/services implement this seam. The HTTP adapter never
/// reaches into cognition, the session journal or provider credentials directly.
#[async_trait]
pub trait Host: Send + Sync {
    async fn operator_events(
        &self,
        _principal: Principal,
        _query: OperatorQuery,
    ) -> ApiResult<EventStream> {
        Err(ApiError::not_found())
    }
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
    Operator(OperatorQuery),
    ModelUsage,
    SubscriptionLimits,
    Credential(CredentialOperation),
    Connectors,
    Telegram(TelegramOperation),
    OpenAiLogin(OpenAiLoginOperation),
    OpenAiSubscription(SubscriptionOperation),
    Profile(ProfileOperation),
    Session(SessionOperation),
    Model(ModelOperation),
    Mcp(McpOperation),
    Memory(MemoryOperation),
    Skill(SkillOperation),
    Config(ConfigOperation),
    Approval(ApprovalOperation),
}
pub enum CredentialOperation {
    List {
        session_id: Option<String>,
    },
    Create(CredentialCreate),
    Get {
        id: String,
    },
    Cancel {
        id: String,
    },
    Submit {
        id: String,
        input: CredentialSubmission,
    },
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
    Delete {
        id: String,
        revision: i64,
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
    Role(ModelRoleOperation),
}
pub enum ModelRoleOperation {
    Create(ModelRoleWrite),
    Update {
        id: String,
        input: ModelRoleWrite,
    },
    Delete {
        id: String,
        revision: Option<String>,
    },
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
    Filtered(ApprovalListQuery),
    Get { id: String, details: bool },
    Decide { id: String, input: ApprovalDecision },
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum HostResponse {
    Operator(OperatorSnapshot),
    ModelUsage(ModelUsageSummary),
    SubscriptionLimits(SubscriptionLimitsView),
    Memory(MemoryView),
    McpServers(McpServerList),
    McpTools(McpToolList),
    CredentialRequests(CredentialRequestList),
    CredentialRequest(CredentialRequestView),
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
