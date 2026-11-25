//! Native tool assembly and execution of one scoped model loop.

use super::types::TurnTools;

use octo_code::{EditTool, GlobTool, GrepTool, ListTool, ReadTool, WriteTool};

use rig::{
    agent::AgentBuilder, agent::NoToolConfig, completion::CompletionModel, completion::Message,
    completion::Prompt, completion::PromptError, tool::ToolDyn,
};
use tracing::debug;

use crate::cogitator::{
    attachment_tool::GetAttachment, retention::fence_tools, retention::permitted_memory,
    retention::temporary, AlbertCogitator,
};
use crate::status::StatusFeed;

impl AlbertCogitator {
    /// Attach the toolset (kaeru memory + dispatch + scratchpad + skills) to a
    /// fresh agent builder and run one bounded tool-loop. Generic over the model
    /// so both auth modes share it; `install` takes the no-tools-yet builder, so
    /// it runs before the dispatch/scratchpad tools are chained on. Returns the
    /// raw loop error so the caller can decide (retry a revoked-token 401, or
    /// classify it for the pool).
    pub(in crate::cogitator) async fn drive<M>(
        &self,
        base: AgentBuilder<M, (), NoToolConfig>,
        tools: TurnTools,
        channel: &str,
        prompt: Message,
        history: Vec<Message>,
        feed: StatusFeed,
    ) -> Result<String, PromptError>
    where
        M: CompletionModel + 'static,
    {
        let TurnTools {
            mut dispatch,
            send_file,
            restart,
            selfconfig,
            discovery,
            children,
            artifacts,
            pad_scope,
            credential,
            confirmation,
            mcp,
        } = tools;
        let pad = self.scratchpad.scoped_handle(pad_scope.clone());
        debug!(
            channel,
            clouds = !self.config.clouds.is_empty(),
            max_turns = self.config.max_tool_turns,
            "building agent + running tool-loop"
        );
        let messaging = dispatch.messaging.take();
        let session = dispatch.session.clone();
        let volatile = temporary(session.as_ref());
        let mut tools = permitted_memory(self.memory.delegation_tools(), volatile);
        feed.register_memory_tool_names(tools.iter().map(|tool| tool.name()));
        tools.extend(vec![
            Box::new(dispatch) as Box<dyn ToolDyn>,
            Box::new(discovery),
            Box::new(pad.goal()),
            Box::new(pad.step()),
            Box::new(pad.mark()),
            Box::new(pad.note()),
            Box::new(pad.clear()),
            Box::new(pad.plan()),
            Box::new(self.skills.list_tool()),
            Box::new(self.skills.search_tool()),
            Box::new(self.skills.apply_tool()),
            Box::new(self.skills.file_tool()),
            Box::new(ReadTool),
            Box::new(WriteTool),
            Box::new(EditTool),
            Box::new(ListTool),
            Box::new(GlobTool),
            Box::new(GrepTool),
        ]);
        tools.extend(
            mcp.into_iter()
                .map(|tool| Box::new(tool) as Box<dyn ToolDyn>),
        );
        if let Some(tool) = confirmation {
            tools.push(Box::new(tool));
        }
        if let Some(tool) = credential {
            tools.push(Box::new(tool));
        }
        if let Some(tool) = messaging {
            tools.push(Box::new(tool));
        }
        if pad_scope.owner && !volatile {
            if let (Some(service), Some(session)) = (self.session_service.clone(), session.clone())
            {
                tools.push(Box::new(GetAttachment {
                    service,
                    session,
                    workspace: self.config.code_workspace.clone(),
                    allowed_ids: None,
                }));
            }
        }
        if let Some(tool) = send_file {
            tools.push(Box::new(tool));
        }
        if let Some(tool) = restart {
            tools.push(Box::new(tool));
        }
        if let Some(sc) = selfconfig {
            tools.extend(vec![
                Box::new(self.models.select_tool()) as Box<dyn ToolDyn>,
                Box::new(sc.read_tool()),
                Box::new(sc.list_tool()),
                Box::new(sc.write_tool()),
                Box::new(sc.edit_tool()),
                Box::new(sc.set_secret_tool()),
                Box::new(sc.list_secrets_tool()),
            ]);
        }
        if let Some(tool) = children {
            tools.push(Box::new(tool));
        }
        let artifacts = if volatile {
            artifacts.without_disk()
        } else {
            artifacts
        };
        let tools = fence_tools(artifacts.wrap(tools), session);
        let stage_store = self.scratchpad.clone();
        let stage_scope = pad_scope.clone();
        let feed = feed.with_stage_source(move || stage_store.progress_scoped(&stage_scope));
        feed.register_tool_names(tools.iter().map(|tool| tool.name()));
        let agent = base.tools(tools).build();
        agent
            .prompt(prompt)
            .with_tool_concurrency(1)
            .with_hook(feed)
            .max_turns(self.config.max_tool_turns)
            .with_history(history)
            .await
    }
}
