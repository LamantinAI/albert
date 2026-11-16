use std::sync::OnceLock;

use super::memory::{
    MemoryBackend, MemoryDocument, MemoryInitialization, MemoryPhase, MemorySettings,
    MemoryTransport, MemoryView, MemoryWrite,
};
use super::models::{
    ModelUsageSummary, ModelUsageView, OperatorCard, OperatorQuery, OperatorSnapshot,
    SubscriptionLimitsView, SubscriptionWindow,
};
use serde_json::{to_value, Value};
use utoipa::openapi::security::{ApiKey, ApiKeyValue, SecurityScheme};
use utoipa::openapi::{OpenApi as Document, Server};
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;

use super::models::{
    AttachmentView, CompactCreate, ComposerSkill, ComposerView, ConnectorList, ConnectorTypeView,
    ConnectorView, CredentialCreate, CredentialRequestList, CredentialRequestView,
    CredentialSubmission, SelectedSkillView, SkillSelection,
};
use super::models::{
    McpConnection, McpServerConfig, McpServerList, McpServerView, McpServerWrite, McpToolList,
    McpToolView,
};

use super::models::{
    TelegramBindingView, TelegramConnectionList, TelegramConnectionView, TelegramCredentialWrite,
    TelegramLoginTokenView, TelegramTokenRequest,
};

use super::{
    models::AccountRole, models::AccountView, models::ApiErrorBody, models::ApiErrorDetail,
    models::ApplyRequest, models::ApplyView, models::ApprovalDecision, models::ApprovalKind,
    models::ApprovalList, models::ApprovalState, models::ApprovalView, models::AttemptView,
    models::CancelRequest, models::Capabilities, models::ClientContext, models::ConfigDocument,
    models::ConfigDraftInput, models::ConfigDraftView, models::ConfigField,
    models::ConfigFieldChange, models::ConfigFieldKind, models::ConfigList, models::ConfigWrite,
    models::CredentialUpdate, models::CredentialView, models::EmptyObject, models::IdentityList,
    models::IdentityView, models::LoginRequest, models::MessageCreate, models::MessageList,
    models::MessageReceipt, models::MessageView, models::ModelConfig, models::ModelOrder,
    models::ModelPoolView, models::ModelPreference, models::ModelRole, models::ModelRoleWrite,
    models::ModelView, models::ModelWrite, models::OpenAiLoginPhase, models::OpenAiLoginStart,
    models::OpenAiLoginSubmission, models::OpenAiLoginView, models::PageQuery, models::ProfileView,
    models::ProgressActivity, models::ProgressDelta, models::ProgressItem,
    models::ProgressSnapshot, models::RefreshView, models::RuntimeStatus, models::SecretList,
    models::SecretWrite, models::SessionCreate, models::SessionEvent, models::SessionList,
    models::SessionStart, models::SessionStartView, models::SessionState, models::SessionUpdate,
    models::SessionView, models::SetupPhase, models::SetupView, models::SkillImport,
    models::SkillList, models::SkillUpdate, models::SkillView, models::SkillWrite,
    models::SubscriptionApplyRequest, models::SubscriptionCatalogModel,
    models::SubscriptionCatalogView, models::SubscriptionConnectionView, models::SubscriptionPhase,
    models::SubscriptionSelection, models::TextDelta, models::TextSnapshot, models::UserCreate,
    models::UserList, models::UserUpdate, models::ValidationView,
};

#[derive(OpenApi)]
#[openapi(components(schemas(
    OperatorQuery,
    OperatorCard,
    OperatorSnapshot,
    ModelUsageView,
    ModelUsageSummary,
    SubscriptionWindow,
    SubscriptionLimitsView,
    MemoryBackend,
    MemoryDocument,
    MemoryTransport,
    MemorySettings,
    MemoryWrite,
    MemoryInitialization,
    MemoryPhase,
    MemoryView,
    ComposerView,
    CredentialCreate,
    CredentialSubmission,
    CredentialRequestView,
    CredentialRequestList,
    ComposerSkill,
    SkillSelection,
    SelectedSkillView,
    CompactCreate,
    AttachmentView,
    ConnectorView,
    ConnectorList,
    ConnectorTypeView,
    TelegramCredentialWrite,
    TelegramTokenRequest,
    TelegramLoginTokenView,
    TelegramBindingView,
    TelegramConnectionView,
    TelegramConnectionList,
    SubscriptionApplyRequest,
    SubscriptionSelection,
    SubscriptionConnectionView,
    SubscriptionPhase,
    SubscriptionCatalogView,
    SubscriptionCatalogModel,
    OpenAiLoginStart,
    OpenAiLoginSubmission,
    OpenAiLoginPhase,
    OpenAiLoginView,
    AccountRole,
    AccountView,
    Capabilities,
    SetupPhase,
    SetupView,
    ClientContext,
    LoginRequest,
    CredentialUpdate,
    CredentialView,
    RefreshView,
    UserCreate,
    UserUpdate,
    UserList,
    ProfileView,
    IdentityView,
    IdentityList,
    ApiErrorBody,
    ApiErrorDetail,
    SessionView,
    SessionStart,
    SessionStartView,
    SessionList,
    SessionCreate,
    SessionUpdate,
    MessageCreate,
    MessageView,
    MessageList,
    MessageReceipt,
    AttemptView,
    SessionState,
    ProgressDelta,
    ProgressActivity,
    ProgressItem,
    ProgressSnapshot,
    TextDelta,
    TextSnapshot,
    SessionEvent,
    CancelRequest,
    PageQuery,
    McpConnection,
    McpServerConfig,
    McpServerView,
    McpServerList,
    McpServerWrite,
    McpToolView,
    McpToolList,
    ModelConfig,
    ModelView,
    ModelPoolView,
    ModelRole,
    ModelRoleWrite,
    ModelWrite,
    ModelOrder,
    ModelPreference,
    SkillView,
    SkillList,
    SkillWrite,
    SkillImport,
    SkillUpdate,
    ConfigList,
    ConfigDocument,
    ConfigWrite,
    ConfigDraftInput,
    ConfigDraftView,
    ConfigField,
    ConfigFieldChange,
    ConfigFieldKind,
    ValidationView,
    ApplyRequest,
    ApplyView,
    SecretWrite,
    SecretList,
    ApprovalView,
    ApprovalKind,
    ApprovalState,
    ApprovalList,
    ApprovalDecision,
    RuntimeStatus,
    EmptyObject
)))]
struct ApiDoc;

/// Schema and executable routes are built from the same annotated domain handlers.
fn document() -> &'static Document {
    static DOCUMENT: OnceLock<Document> = OnceLock::new();
    DOCUMENT.get_or_init(|| {
        let (_, mut document) = OpenApiRouter::with_openapi(ApiDoc::openapi())
            .merge(super::router::domain_routes(32 * 1024 * 1024))
            .split_for_parts();
        document.info.title = "Albert Web Panel API".into();
        document.info.version = "1".into();
        document.servers = Some(vec![Server::new(".")]);
        let components = document.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "browserCookie",
            SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new("albert_auth"))),
        );
        components.add_security_scheme(
            "refreshCookie",
            SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new("albert_refresh"))),
        );
        document
    })
}
pub fn schema() -> Value {
    to_value(document()).expect("OpenAPI serializes")
}
pub(crate) fn paths() -> impl Iterator<Item = &'static str> {
    document().paths.paths.keys().map(String::as_str)
}
