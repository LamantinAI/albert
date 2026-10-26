use std::path::PathBuf;

use rig::tool::ToolDyn;

use super::super::retention::{permitted_memory, temporary};
use super::super::AlbertCogitator;
use crate::{
    scratchpad::ScratchpadStore,
    sessions::SessionContext,
    subagents::{WorkspaceRead, WorkspaceWrite},
};

impl AlbertCogitator {
    pub(super) fn child_tools_for(
        &self,
        workspace: PathBuf,
        session: Option<&SessionContext>,
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
        tools.extend(permitted_memory(
            self.memory.delegation_tools(),
            temporary(session),
        ));
        tools
    }
}
