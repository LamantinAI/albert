use serde_json::{json, to_value, Value};
use utoipa::OpenApi;

use super::models::{
    AccountRole, AccountView, ApiErrorBody, ApiErrorDetail, ApplyRequest, ApplyView,
    ApprovalDecision, ApprovalList, ApprovalView, AttemptView, CancelRequest, Capabilities,
    ClientContext, ConfigDocument, ConfigList, ConfigWrite, CredentialUpdate, CredentialView,
    EmptyObject, IdentityList, IdentityView, LoginRequest, MessageCreate, MessageList,
    MessageReceipt, MessageView, ModelConfig, ModelOrder, ModelPoolView, ModelPreference,
    ModelRole, ModelView, ModelWrite, PageQuery, ProfileView, RuntimeStatus, SecretList,
    SecretWrite, SessionCreate, SessionEvent, SessionList, SessionState, SessionUpdate,
    SessionView, SetupPhase, SetupView, SkillImport, SkillList, SkillUpdate, SkillView, SkillWrite,
    UserCreate, UserList, UserUpdate, ValidationView,
};

#[derive(OpenApi)]
#[openapi(components(schemas(
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
    SessionList,
    SessionCreate,
    SessionUpdate,
    MessageCreate,
    MessageView,
    MessageList,
    MessageReceipt,
    AttemptView,
    SessionState,
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
    document["servers"] = json!([{"url":"/api/v1"}]);
    document["components"]["securitySchemes"] =
        json!({"browserCookie":{"type":"apiKey","in":"cookie","name":"albert_auth"}});
    for &(method, path, request, response) in ROUTES {
        let mut operation = json!({"responses":{"200":{"description":"Success","content":{"application/json":{"schema":{"$ref":format!("#/components/schemas/{response}")}}}},"default":{"description":"Error","content":{"application/json":{"schema":{"$ref":"#/components/schemas/ApiErrorBody"}}}}}});
        if let Some(request) = request {
            operation["requestBody"] = json!({"required":true,"content":{"application/json":{"schema":{"$ref":format!("#/components/schemas/{request}")}}}});
        }
        let mut parameters = Vec::new();
        for segment in path.split('/') {
            if let Some(name) = segment.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                parameters.push(
                    json!({"name":name,"in":"path","required":true,"schema":{"type":"string"}}),
                );
            }
        }
        if method != "GET" && path != "/auth/credentials" {
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
