use std::path::PathBuf;

use rig::tool::ToolDyn;

use super::{
    super::retention::permitted_memory, super::retention::temporary, super::AlbertCogitator,
};
use crate::{
    scratchpad::ScratchpadStore, sessions::SessionContext, status::StatusFeed,
    subagents::WorkspaceRead, subagents::WorkspaceWrite,
};

impl AlbertCogitator {
    pub(super) fn child_tools_for(
        &self,
        workspace: PathBuf,
        session: Option<&SessionContext>,
    ) -> Vec<Box<dyn ToolDyn>> {
        self.child_tools_with_progress(workspace, session, None)
    }

    pub(super) fn child_tools_with_progress(
        &self,
        workspace: PathBuf,
        session: Option<&SessionContext>,
        feed: Option<&StatusFeed>,
    ) -> Vec<Box<dyn ToolDyn>> {
        let pad = ScratchpadStore::with_limits(self.config.scratchpad.clone())
            .expect("validated scratchpad limits")
            .handle("child");
        let mut tools: Vec<Box<dyn ToolDyn>> = vec![
            Box::new(pad.goal()),
            Box::new(pad.step()),
            Box::new(pad.mark()),
            Box::new(pad.note()),
            Box::new(pad.clear()),
            Box::new(self.skills.list_tool()),
            Box::new(self.skills.search_tool()),
            Box::new(self.skills.apply_tool()),
            Box::new(self.skills.file_tool()),
            Box::new(WorkspaceRead(workspace.clone())),
            Box::new(WorkspaceWrite(workspace)),
        ];
        let memory = permitted_memory(self.memory.delegation_tools(), temporary(session));
        if let Some(feed) = feed {
            feed.register_memory_tool_names(memory.iter().map(|tool| tool.name()));
        }
        tools.extend(memory);
        tools
    }
}
