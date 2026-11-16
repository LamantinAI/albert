//! Checked projection from the host facade into each handler's response DTO.
use crate::api::models::{ModelUsageSummary, OperatorSnapshot, SubscriptionLimitsView};
use axum::Json;

use super::authorization::host_access;
use crate::api::memory::MemoryView;
use crate::api::models::{
    ApplyView, ApprovalList, ApprovalView, AttemptView, ComposerView, ConfigDocument,
    ConfigDraftView, ConfigList, ConnectorList, CredentialRequestList, CredentialRequestView,
    EmptyObject, IdentityList, McpServerList, McpToolList, MessageList, MessageReceipt,
    ModelPoolView, OpenAiLoginView, ProfileView, SecretList, SessionList, SessionStartView,
    SessionState, SessionView, SkillList, SkillView, SubscriptionCatalogView,
    SubscriptionConnectionView, TelegramConnectionList, TelegramConnectionView,
    TelegramLoginTokenView, ValidationView,
};
use crate::api::{
    errors::ApiError, errors::ApiResult, host::HostOperation, host::HostResponse,
    principal::Principal, state::ApiState,
};

pub trait HostView: Sized {
    fn extract(response: HostResponse) -> ApiResult<Self>;
}
macro_rules! views {
    ($($view:ty => $variant:ident),* $(,)?) => {$ (
        impl HostView for $view {
            fn extract(response: HostResponse) -> ApiResult<Self> {
                match response { HostResponse::$variant(value) => Ok(value), _ => Err(ApiError::internal()) }
            }
        }
    )*};
}
views! {
    OperatorSnapshot => Operator,
    ModelUsageSummary => ModelUsage,
    SubscriptionLimitsView => SubscriptionLimits,
    MemoryView => Memory,
    McpServerList => McpServers,
    McpToolList => McpTools,
    CredentialRequestList => CredentialRequests,
    CredentialRequestView => CredentialRequest,
    ComposerView => Composer,
    ConnectorList => Connectors,
    TelegramConnectionList => TelegramConnections,
    TelegramConnectionView => TelegramConnection,
    TelegramLoginTokenView => TelegramToken,
    OpenAiLoginView => OpenAiLogin,
    SubscriptionConnectionView => SubscriptionConnection,
    SubscriptionCatalogView => SubscriptionCatalog,
    ProfileView => Profile,
    IdentityList => Identities,
    SessionView => Session,
    SessionStartView => Start,
    SessionList => Sessions,
    MessageList => Messages,
    MessageReceipt => Receipt,
    SessionState => State,
    AttemptView => Attempt,
    ModelPoolView => Models,
    SkillView => Skill,
    SkillList => Skills,
    ConfigDocument => Config,
    ConfigDraftView => ConfigDraft,
    ConfigList => Configs,
    ValidationView => Validation,
    ApplyView => Apply,
    SecretList => Secrets,
    ApprovalView => Approval,
    ApprovalList => Approvals,
    EmptyObject => Empty,
}

pub async fn run<T: HostView>(
    state: &ApiState,
    caller: &Principal,
    operation: HostOperation,
) -> ApiResult<Json<T>> {
    host_access(caller, &operation)?;
    T::extract(state.host.operation(caller, operation).await?).map(Json)
}
