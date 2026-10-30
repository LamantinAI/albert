use std::sync::Arc;

use octo_rig::{RestartTool, SendFileTool};
use rig::tool::ToolDyn;

use crate::{
    artifacts::Artifacts,
    cogitator::session_dispatch::SessionDispatch,
    connector_catalog::ConnectorCatalog,
    scratchpad::PadScope,
    selfconfig::SelfConfig,
    subagents::{budget::Budget, SubagentTool},
};

/// The per-attempt tool instances for one drive run (dispatch + the optional
/// send-file / owner-only restart and self-config tools). A run consumes them, so
/// each retry builds a fresh set.
type TurnTools = (
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
