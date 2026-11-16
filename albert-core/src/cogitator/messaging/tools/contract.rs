use crate::{
    messaging::contract::ContextAttachment, messaging::contract::DelegationCeiling,
    messaging::contract::DestinationSpec, messaging::contract::ExecutionLocation,
    messaging::contract::MessageId, messaging::contract::MessageIntent,
    messaging::contract::WorkReportPolicy,
};
use octo_history::SessionId;
use serde::Deserialize;
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::cogitator) enum MessagingAction {
    Discover {
        limit: Option<usize>,
        before_id: Option<i64>,
        through_id: Option<i64>,
        route_offset: Option<usize>,
    },
    Inspect {
        session_id: SessionId,
        limit: Option<usize>,
        before_id: Option<i64>,
        through_id: Option<i64>,
        record_id: Option<i64>,
        text_offset: Option<usize>,
    },
    Send {
        #[serde(flatten)]
        message: SendProposal,
    },
    Status {
        message_id: MessageId,
    },
    Cancel {
        message_id: MessageId,
    },
    Steer {
        message_id: MessageId,
        content: String,
    },
    Report {
        content: String,
    },
    Reports {
        message_id: Option<MessageId>,
        policy: Option<WorkReportPolicy>,
    },
    Execution {
        location: Option<ExecutionLocation>,
        expected_revision: Option<i64>,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::cogitator) struct SendProposal {
    pub(super) destination: DestinationSpec,
    pub(super) intent: MessageIntent,
    pub(super) content: String,
    pub(super) timeout_secs: Option<u64>,
    pub(super) ceiling: Option<DelegationCeiling>,
    pub(super) policy: Option<WorkReportPolicy>,
    #[serde(default)]
    pub(super) context: Vec<ContextAttachment>,
}
