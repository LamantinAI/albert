mod composer;
pub use composer::{CompactCreate, ComposerSkill, ComposerView, SelectedSkillView, SkillSelection};
mod media;
pub use media::AttachmentView;
pub mod connectors;
pub use connectors::{ConnectorList, ConnectorTypeView, ConnectorView};
pub mod telegram;
pub use telegram::{
    TelegramBindingView, TelegramConnectionList, TelegramConnectionView, TelegramCredentialWrite,
    TelegramLoginTokenView, TelegramTokenRequest,
};
pub mod account;
pub mod configuration;
pub mod control;
pub mod progress;
pub mod session;
pub mod start;
pub mod text;
pub use start::{SessionStart, SessionStartView};
pub use text::{TextDelta, TextSnapshot};

pub use account::{
    AccountRole, AccountView, ApiErrorBody, ApiErrorDetail, Capabilities, ClientContext,
    CredentialUpdate, CredentialView, IdentityList, IdentityView, LoginRequest, ProfileView,
    SetupPhase, SetupView, UserCreate, UserList, UserUpdate,
};
pub use configuration::{
    ConfigDraftInput, ConfigDraftView, ConfigField, ConfigFieldChange, ConfigFieldKind,
};
pub use control::{
    ApplyRequest, ApplyView, ApprovalDecision, ApprovalList, ApprovalView, ConfigDocument,
    ConfigList, ConfigWrite, EmptyObject, ModelConfig, ModelOrder, ModelPoolView, ModelPreference,
    ModelRole, ModelView, ModelWrite, RuntimeStatus, SecretList, SecretWrite, SkillImport,
    SkillList, SkillUpdate, SkillView, SkillWrite, ValidationView,
};
pub use progress::{ProgressDelta, ProgressItem, ProgressSnapshot};
mod progress_activity;
pub use progress_activity::ProgressActivity;
pub use session::{
    AttemptView, CancelRequest, MessageCreate, MessageList, MessageReceipt, MessageView, PageQuery,
    SessionCreate, SessionEvent, SessionList, SessionState, SessionUpdate, SessionView,
};

pub mod openai_login;
pub use openai_login::{
    OpenAiLoginPhase, OpenAiLoginStart, OpenAiLoginSubmission, OpenAiLoginView,
};

pub use openai_login::{
    SubscriptionApplyRequest, SubscriptionCatalogModel, SubscriptionCatalogView,
    SubscriptionConnectionView, SubscriptionPhase, SubscriptionSelection,
};

#[cfg(test)]
mod tests {
    use albert_core::{
        status::domain::ProgressActivity as DomainProgressActivity,
        status::domain::ProgressDelta as DomainProgressDelta,
        status::domain::ProgressItem as DomainProgressItem,
        status::domain::TextDelta as DomainTextDelta,
    };
    use serde_json::{from_value, json, to_value};

    use super::{ProgressActivity, ProgressDelta, ProgressItem, TextDelta};

    #[test]
    fn activity_metadata_is_optional_and_future_categories_remain_generic() {
        let mut wire = json!({"id":"call", "kind":"tool", "label":"Memory", "state":"running", "started_at":"timestamp", "updated_at":"timestamp", "elapsed_ms":null});
        let legacy: ProgressItem = from_value(wire.clone()).unwrap();
        assert_eq!(legacy.activity, None);
        assert_eq!(to_value(legacy).unwrap(), wire);
        wire["activity"] = json!("future_category");
        let future: ProgressItem = from_value(wire).unwrap();
        assert_eq!(future.activity, Some(ProgressActivity::Unknown));
        assert_eq!(to_value(future).unwrap()["activity"], "unknown");
        for category in [
            DomainProgressActivity::Memory,
            DomainProgressActivity::Scratchpad,
            DomainProgressActivity::Skill,
            DomainProgressActivity::Connector,
            DomainProgressActivity::Search,
            DomainProgressActivity::Browser,
            DomainProgressActivity::Files,
            DomainProgressActivity::Shell,
            DomainProgressActivity::Sql,
            DomainProgressActivity::Calendar,
            DomainProgressActivity::Mail,
            DomainProgressActivity::Image,
            DomainProgressActivity::AudioInput,
            DomainProgressActivity::AudioOutput,
            DomainProgressActivity::Subagent,
            DomainProgressActivity::Sessions,
            DomainProgressActivity::Configuration,
            DomainProgressActivity::Model,
            DomainProgressActivity::Unknown,
        ] {
            assert_eq!(
                to_value(category).unwrap(),
                to_value(ProgressActivity::from(category)).unwrap()
            );
        }
    }

    #[test]
    fn live_status_http_mapping_preserves_event_replay_json() {
        let progress = DomainProgressDelta {
            attempt_id: "attempt".into(),
            at: "timestamp".into(),
            replace_kind: Some("tools".into()),
            items: vec![DomainProgressItem {
                id: "tool".into(),
                kind: "tool".into(),
                label: "Search".into(),
                state: "running".into(),
                started_at: "timestamp".into(),
                updated_at: "timestamp".into(),
                elapsed_ms: Some(0),
                activity: Some(DomainProgressActivity::Search),
            }],
        };
        assert_eq!(
            to_value(&progress).unwrap(),
            to_value(ProgressDelta::from(progress)).unwrap()
        );
        let text = DomainTextDelta {
            attempt_id: "attempt".into(),
            generation: "generation".into(),
            operation: "append".into(),
            offset: 4,
            text: "text".into(),
            message_ids: vec!["message".into()],
        };
        assert_eq!(
            to_value(&text).unwrap(),
            to_value(TextDelta::from(text)).unwrap()
        );
    }
}
