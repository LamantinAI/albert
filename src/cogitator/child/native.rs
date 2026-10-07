use std::path::PathBuf;

use rig::tool::ToolDyn;

use super::super::AlbertCogitator;
use crate::{
    scratchpad::ScratchpadStore,
    subagents::{WorkspaceRead, WorkspaceWrite},
};

impl AlbertCogitator {
    pub(super) fn child_native_tools(&self, workspace: PathBuf) -> Vec<Box<dyn ToolDyn>> {
        let pad = ScratchpadStore::new().handle("child");
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
        tools.extend(self.memory.delegation_tools());
        tools
    }
}
