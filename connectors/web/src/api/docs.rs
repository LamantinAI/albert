use serde_json::{json, to_value, Value};
use utoipa::OpenApi;

use super::models::{
    AttachmentView, CompactCreate, ComposerSkill, ComposerView, ConnectorList, ConnectorTypeView,
    ConnectorView, SelectedSkillView, SkillSelection,
};

use super::models::{
    TelegramBindingView, TelegramConnectionList, TelegramConnectionView, TelegramCredentialWrite,
    TelegramLoginTokenView, TelegramTokenRequest,
};

use super::{
    models::AccountRole, models::AccountView, models::ApiErrorBody, models::ApiErrorDetail,
    models::ApplyRequest, models::ApplyView, models::ApprovalDecision, models::ApprovalList,
    models::ApprovalView, models::AttemptView, models::CancelRequest, models::Capabilities,
    models::ClientContext, models::ConfigDocument, models::ConfigDraftInput,
    models::ConfigDraftView, models::ConfigField, models::ConfigFieldChange,
    models::ConfigFieldKind, models::ConfigList, models::ConfigWrite, models::CredentialUpdate,
    models::CredentialView, models::EmptyObject, models::IdentityList, models::IdentityView,
    models::LoginRequest, models::MessageCreate, models::MessageList, models::MessageReceipt,
    models::MessageView, models::ModelConfig, models::ModelOrder, models::ModelPoolView,
    models::ModelPreference, models::ModelRole, models::ModelView, models::ModelWrite,
    models::OpenAiLoginPhase, models::OpenAiLoginStart, models::OpenAiLoginSubmission,
    models::OpenAiLoginView, models::PageQuery, models::ProfileView, models::ProgressActivity,
    models::ProgressDelta, models::ProgressItem, models::ProgressSnapshot, models::RuntimeStatus,
    models::SecretList, models::SecretWrite, models::SessionCreate, models::SessionEvent,
    models::SessionList, models::SessionStart, models::SessionStartView, models::SessionState,
    models::SessionUpdate, models::SessionView, models::SetupPhase, models::SetupView,
    models::SkillImport, models::SkillList, models::SkillUpdate, models::SkillView,
    models::SkillWrite, models::SubscriptionApplyRequest, models::SubscriptionCatalogModel,
    models::SubscriptionCatalogView, models::SubscriptionConnectionView, models::SubscriptionPhase,
    models::SubscriptionSelection, models::TextDelta, models::TextSnapshot, models::UserCreate,
    models::UserList, models::UserUpdate, models::ValidationView,
};

#[derive(OpenApi)]
#[openapi(components(schemas(
    ComposerView,
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
    ModelConfig,
    ModelView,
    ModelPoolView,
    ModelRole,
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
    ApprovalList,
    ApprovalDecision,
    RuntimeStatus,
    EmptyObject
)))]
struct ApiDoc;

/// This route inventory also drives router registration; request/response shapes
/// come from Rust DTO schemas, not a second handwritten TypeScript contract.
pub const ROUTES: &[(&str, &str, Option<&str>, &str)] = &[
    ("GET", "/composer", None, "ComposerView"),
    (
        "POST",
        "/sessions/{id}/compactions",
        Some("CompactCreate"),
        "MessageReceipt",
    ),
    ("POST", "/attachments", None, "AttachmentView"),
    ("DELETE", "/attachments/{id}", None, "EmptyObject"),
    (
        "GET",
        "/sessions/{id}/attachments/{asset_id}",
        None,
        "AttachmentView",
    ),
    (
        "HEAD",
        "/sessions/{id}/attachments/{asset_id}",
        None,
        "AttachmentView",
    ),
    ("GET", "/connectors", None, "ConnectorList"),
    (
        "GET",
        "/account/telegram/connections",
        None,
        "TelegramConnectionList",
    ),
    (
        "GET",
        "/account/connectors/{id}/telegram",
        None,
        "TelegramConnectionView",
    ),
    (
        "PUT",
        "/connectors/{id}/telegram/credential",
        Some("TelegramCredentialWrite"),
        "TelegramConnectionView",
    ),
    (
        "POST",
        "/account/connectors/{id}/telegram/login-tokens",
        Some("TelegramTokenRequest"),
        "TelegramLoginTokenView",
    ),
    (
        "DELETE",
        "/account/connectors/{id}/telegram/login-tokens/current",
        None,
        "EmptyObject",
    ),
    (
        "DELETE",
        "/account/connectors/{id}/telegram/binding",
        None,
        "TelegramConnectionView",
    ),
    (
        "POST",
        "/openai-login-attempts",
        Some("OpenAiLoginStart"),
        "OpenAiLoginView",
    ),
    (
        "GET",
        "/openai-login-attempts/{id}",
        None,
        "OpenAiLoginView",
    ),
    (
        "POST",
        "/openai-login-attempts/{id}/submissions",
        Some("OpenAiLoginSubmission"),
        "OpenAiLoginView",
    ),
    (
        "POST",
        "/openai-login-attempts/{id}/cancellations",
        None,
        "OpenAiLoginView",
    ),
    (
        "GET",
        "/openai-subscription",
        None,
        "SubscriptionConnectionView",
    ),
    (
        "POST",
        "/openai-subscription/catalogs",
        None,
        "SubscriptionCatalogView",
    ),
    (
        "POST",
        "/openai-subscription/selections",
        Some("SubscriptionSelection"),
        "SubscriptionConnectionView",
    ),
    (
        "POST",
        "/openai-subscription/applications",
        Some("SubscriptionApplyRequest"),
        "SubscriptionConnectionView",
    ),
    ("GET", "/context", None, "ClientContext"),
    (
        "POST",
        "/auth/credentials",
        Some("LoginRequest"),
        "CredentialView",
    ),
    ("DELETE", "/auth/credentials/{id}", None, "EmptyObject"),
    (
        "PUT",
        "/account/credentials",
        Some("CredentialUpdate"),
        "AccountView",
    ),
    ("GET", "/users", None, "UserList"),
    ("POST", "/users", Some("UserCreate"), "AccountView"),
    ("PATCH", "/users/{id}", Some("UserUpdate"), "AccountView"),
    ("GET", "/account/profile", None, "ProfileView"),
    ("GET", "/account/identities", None, "IdentityList"),
    (
        "POST",
        "/session-starts",
        Some("SessionStart"),
        "SessionStartView",
    ),
    ("GET", "/session-starts/{id}", None, "SessionStartView"),
    (
        "POST",
        "/session-starts/{id}/cancellations",
        Some("EmptyObject"),
        "SessionStartView",
    ),
    ("GET", "/sessions", None, "SessionList"),
    ("POST", "/sessions", Some("SessionCreate"), "SessionView"),
    ("GET", "/sessions/{id}", None, "SessionView"),
    (
        "PATCH",
        "/sessions/{id}",
        Some("SessionUpdate"),
        "SessionView",
    ),
    ("GET", "/sessions/{id}/messages", None, "MessageList"),
    (
        "POST",
        "/sessions/{id}/messages",
        Some("MessageCreate"),
        "MessageReceipt",
    ),
    (
        "GET",
        "/sessions/{id}/messages/{request_id}",
        None,
        "MessageReceipt",
    ),
    ("GET", "/sessions/{id}/state", None, "SessionState"),
    ("GET", "/sessions/{id}/events", None, "SessionEvent"),
    (
        "POST",
        "/attempts/{id}/cancellations",
        Some("CancelRequest"),
        "AttemptView",
    ),
    ("GET", "/model-pool", None, "ModelPoolView"),
    ("POST", "/models", Some("ModelWrite"), "ModelPoolView"),
    ("PUT", "/models/{id}", Some("ModelWrite"), "ModelPoolView"),
    ("DELETE", "/models/{id}", None, "ModelPoolView"),
    (
        "PUT",
        "/model-pool/order",
        Some("ModelOrder"),
        "ModelPoolView",
    ),
    (
        "PUT",
        "/model-pool/preference",
        Some("ModelPreference"),
        "ModelPoolView",
    ),
    ("GET", "/skills", None, "SkillList"),
    ("GET", "/skills/{id}", None, "SkillView"),
    ("PUT", "/skills/{id}", Some("SkillWrite"), "SkillView"),
    ("PATCH", "/skills/{id}", Some("SkillUpdate"), "SkillView"),
    ("DELETE", "/skills/{id}", None, "EmptyObject"),
    ("POST", "/skill-imports", Some("SkillImport"), "SkillView"),
    ("GET", "/configuration", None, "ConfigList"),
    ("GET", "/configuration/{id}", None, "ConfigDocument"),
    (
        "POST",
        "/configuration/{id}/drafts",
        Some("ConfigDraftInput"),
        "ConfigDraftView",
    ),
    (
        "PUT",
        "/configuration/{id}",
        Some("ConfigWrite"),
        "ConfigDocument",
    ),
    (
        "POST",
        "/configuration-validations",
        Some("ConfigWrite"),
        "ValidationView",
    ),
    (
        "POST",
        "/configuration/applications",
        Some("ApplyRequest"),
        "ApplyView",
    ),
    ("GET", "/configuration/applications/{id}", None, "ApplyView"),
    ("GET", "/prompts/{id}", None, "ConfigDocument"),
    (
        "PUT",
        "/prompts/{id}",
        Some("ConfigWrite"),
        "ConfigDocument",
    ),
    ("GET", "/secrets", None, "SecretList"),
    ("PUT", "/secrets/{id}", Some("SecretWrite"), "EmptyObject"),
    ("DELETE", "/secrets/{id}", None, "EmptyObject"),
    ("GET", "/approvals", None, "ApprovalList"),
    (
        "POST",
        "/approvals/{id}/decisions",
        Some("ApprovalDecision"),
        "ApprovalView",
    ),
];

pub fn schema() -> Value {
    let mut document = to_value(ApiDoc::openapi()).expect("OpenAPI serializes");
    document["info"] = json!({"title":"Albert Web Panel API","version":"1"});
    document["servers"] = json!([{"url":"."}]);
    document["components"]["securitySchemes"] =
        json!({"browserCookie":{"type":"apiKey","in":"cookie","name":"albert_auth"}});
    for &(method, path, request, response) in ROUTES {
        let mut operation = json!({"responses":{"200":{"description":"Success","content":{"application/json":{"schema":{"$ref":format!("#/components/schemas/{response}")}}}},"default":{"description":"Error","content":{"application/json":{"schema":{"$ref":"#/components/schemas/ApiErrorBody"}}}}}});
        if let Some(request) = request {
            operation["requestBody"] = json!({"required":true,"content":{"application/json":{"schema":{"$ref":format!("#/components/schemas/{request}")}}}});
        }
        if path == "/attachments" && method == "POST" {
            operation["requestBody"] = json!({"required":true,"content":{"application/octet-stream":{"schema":{"type":"string","format":"binary"}}}});
        }
        if path.ends_with("/attachments/{asset_id}") {
            operation["responses"]["200"]["content"] =
                json!({"application/octet-stream":{"schema":{"type":"string","format":"binary"}}});
            operation["responses"]["206"] = operation["responses"]["200"].clone();
            operation["responses"]["416"] =
                json!({"description":"Requested range is not satisfiable"});
        }
        let mut parameters = Vec::new();
        for segment in path.split('/') {
            if let Some(name) = segment.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                parameters.push(
                    json!({"name":name,"in":"path","required":true,"schema":{"type":"string"}}),
                );
            }
        }
        if !matches!(method, "GET" | "HEAD") && path != "/auth/credentials" {
            parameters.push(json!({"name":"x-albert-csrf","in":"header","required":true,"schema":{"type":"string"}}));
        }
        operation["parameters"] = json!(parameters);
        operation["security"] = if path == "/auth/credentials" || path == "/context" {
            json!([])
        } else {
            json!([{"browserCookie":[]}])
        };
        document["paths"][path][method.to_ascii_lowercase()] = operation;
    }
    document
}
