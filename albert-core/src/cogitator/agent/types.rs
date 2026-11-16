use crate::mcp::McpTool;
use std::sync::Arc;

use octo_rig::RestartTool;
use rig::tool::ToolDyn;

use crate::{
    artifacts::Artifacts, cogitator::media_delivery::MediaDelivery,
    cogitator::session_dispatch::SessionDispatch, confirmations::tool::RequestConfirmation,
    connector_catalog::ConnectorCatalog, credentials::tool::RequestCredential,
    scratchpad::PadScope, selfconfig::SelfConfig, sessions::SessionContext,
    subagents::budget::Budget, subagents::SubagentTool,
};

/// Fresh tool instances for each model attempt.
pub(in crate::cogitator) struct TurnTools {
    pub dispatch: SessionDispatch,
    pub send_file: Option<MediaDelivery>,
    pub restart: Option<RestartTool>,
    pub selfconfig: Option<SelfConfig>,
    pub discovery: ConnectorCatalog,
    pub children: Option<SubagentTool>,
    pub artifacts: Artifacts,
    pub pad_scope: PadScope,
    pub credential: Option<RequestCredential>,
    pub confirmation: Option<RequestConfirmation>,
    pub mcp: Vec<McpTool>,
}

pub(in crate::cogitator) enum AttemptTools {
    ChatOnly {
        session: Option<SessionContext>,
        pad: PadScope,
    },
    Metadata {
        max_tokens: usize,
    },
    Compact {
        max_tokens: usize,
    },
    Root(TurnTools),
    Child {
        tools: Vec<Box<dyn ToolDyn>>,
        max_turns: usize,
        budget: Arc<Budget>,
    },
}
