use std::sync::Arc;

use octo_rig::{RestartTool, SendFileTool};
use rig::tool::ToolDyn;

use crate::{
    artifacts::Artifacts,
    cogitator::session_dispatch::SessionDispatch,
    connector_catalog::ConnectorCatalog,
    scratchpad::PadScope,
    selfconfig::SelfConfig,
    sessions::SessionContext,
    subagents::{budget::Budget, SubagentTool},
};

/// Fresh tool instances for each model attempt.
pub(in crate::cogitator) type TurnTools = (
    SessionDispatch,
    Option<SendFileTool>,
    Option<RestartTool>,
    Option<SelfConfig>,
    ConnectorCatalog,
    Option<SubagentTool>,
    Artifacts,
    PadScope,
);

pub(in crate::cogitator) enum AttemptTools {
    ChatOnly {
        session: Option<SessionContext>,
        pad: PadScope,
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
