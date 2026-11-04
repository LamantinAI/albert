//! Versioned data contracts are descriptive, never ingress or tool authority.
use octo_history::{SessionId, SessionJournal};
use rand::random;
use serde::{Deserialize, Serialize};

use super::{MessagingError, MessagingResult};
use crate::sessions::SessionContext;

pub const CONTRACT_VERSION: u16 = 1;
pub const MESSAGE_MARKER: &str = "[albert session messaging v1]\n";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct MessageId(String);
impl MessageId {
    pub fn new() -> Self {
        Self(format!("{:032x}", random::<u128>()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for MessageId {
    type Error = String;
    fn try_from(value: String) -> Result<Self, String> {
        if value.len() != 32 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("invalid message ID".into());
        }
        Ok(Self(value.to_ascii_lowercase()))
    }
}
impl From<MessageId> for String {
    fn from(value: MessageId) -> String {
        value.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "id",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum WorkRef {
    Turn(String),
    Child(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Machine {
    Host,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionLocation {
    pub machine: Machine,
    pub connector: Option<String>,
}
impl Default for ExecutionLocation {
    fn default() -> Self {
        Self {
            machine: Machine::Host,
            connector: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisplayRoute {
    pub connector: String,
    pub channel: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DestinationSpec {
    FixedSession {
        session: SessionId,
        display: Option<DisplayRoute>,
    },
    ChannelActive {
        route: DisplayRoute,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedDestination {
    pub session: SessionId,
    pub display: Option<DisplayRoute>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageIntent {
    Notice,
    Question,
    WorkRequest,
    Response,
}
impl MessageIntent {
    pub fn schedules_work(self) -> bool {
        matches!(self, Self::Question | Self::WorkRequest)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportClass {
    Intermediate,
    Final,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkReportPolicy {
    pub revision: i64,
    pub intermediate: Option<DestinationSpec>,
    pub final_result: Option<DestinationSpec>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReturnAddress {
    pub session: SessionId,
    pub display: DisplayRoute,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContentRef {
    pub journal: SessionJournal,
    pub record_id: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextAttachment {
    pub reference: ContentRef,
    pub through_id: i64,
    pub excerpt: Option<String>,
}

/// Only ordinary connector/kind grants. There is deliberately no owner role,
/// native SQL grant, approval identity or ingress in this contract.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DelegationCeiling {
    pub grants: Vec<ConnectorGrant>,
    pub max_tool_calls: u32,
    pub max_output_bytes: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorGrant {
    pub connector: String,
    pub kinds: Vec<String>,
}

/// Proposal fields contain no actor/source. Host code supplies those from the
/// current authenticated execution context rather than deserializing authority.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageDraft {
    pub id: MessageId,
    pub destination: DestinationSpec,
    pub intent: MessageIntent,
    pub content: String,
    pub correlation: Option<MessageId>,
    pub causation: Option<MessageId>,
    pub deadline_unix_ms: i64,
    pub hops_remaining: u16,
    pub context: Vec<ContextAttachment>,
    pub ceiling: Option<DelegationCeiling>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageEnvelope {
    pub version: u16,
    pub id: MessageId,
    pub actor: String,
    pub source: SessionId,
    pub work: WorkRef,
    pub execution: ExecutionLocation,
    pub intended: DestinationSpec,
    pub intent: MessageIntent,
    pub correlation: Option<MessageId>,
    pub causation: Option<MessageId>,
    pub deadline_unix_ms: i64,
    pub hops_remaining: u16,
    pub return_address: ReturnAddress,
    pub ceiling: Option<DelegationCeiling>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admission_report_policy: Option<WorkReportPolicy>,
}

/// Not serializable. Construct in host tool wiring from the current SessionContext
/// and existing envelope/child ID; never from model-provided source/actor fields.
#[derive(Clone)]
pub struct TrustedSource {
    pub(super) context: SessionContext,
    pub(super) work: WorkRef,
    pub(super) execution: ExecutionLocation,
}
impl TrustedSource {
    pub fn context(&self) -> &SessionContext {
        &self.context
    }
    pub fn work(&self) -> &WorkRef {
        &self.work
    }
    pub fn execution(&self) -> &ExecutionLocation {
        &self.execution
    }

    pub fn current(
        context: &SessionContext,
        work: WorkRef,
        execution: ExecutionLocation,
    ) -> MessagingResult<Self> {
        context.private_user_id()?;
        if !context.is_live() {
            return Err(MessagingError::Unavailable);
        }
        Ok(Self {
            context: context.clone(),
            work,
            execution,
        })
    }
}
