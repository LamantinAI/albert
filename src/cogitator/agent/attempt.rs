use rig::{
    agent::{AgentBuilder, NoToolConfig},
    completion::{CompletionModel, Message, Prompt, PromptError},
};

use super::{chat::chat_tools, AttemptTools};
use crate::{cogitator::AlbertCogitator, status::StatusFeed};

impl AlbertCogitator {
    pub(super) async fn drive_attempt<M: CompletionModel + 'static>(
        &self,
        base: AgentBuilder<M, (), NoToolConfig>,
        tools: AttemptTools,
        channel: &str,
        prompt: Message,
        history: Vec<Message>,
        feed: StatusFeed,
    ) -> Result<String, PromptError> {
        match tools {
            AttemptTools::Root((
                dispatch,
                send_file,
                restart,
                selfconfig,
                discovery,
                children,
                artifacts,
                pad_scope,
            )) => {
                self.drive(
                    base, dispatch, send_file, restart, selfconfig, discovery, children, channel,
                    prompt, history, feed, artifacts, pad_scope,
                )
                .await
            }
            AttemptTools::ChatOnly { session, pad } => {
                base.tools(chat_tools(&self.scratchpad, pad, session))
                    .build()
                    .prompt(prompt)
                    .with_hook(feed)
                    .max_turns(self.config.max_tool_turns)
                    .with_history(history)
                    .await
            }
            AttemptTools::Compact { max_tokens } | AttemptTools::Metadata { max_tokens } => {
                base.max_tokens(max_tokens as u64)
                    .build()
                    .prompt(prompt)
                    .with_history(history)
                    .max_turns(1)
                    .await
            }
            AttemptTools::Child {
                tools, max_turns, ..
            } => {
                feed.register_tool_names(tools.iter().map(|tool| tool.name()));
                base.tools(tools)
                    .build()
                    .prompt(prompt)
                    .with_history(history)
                    .with_hook(feed)
                    .max_turns(max_turns)
                    .await
            }
        }
    }
}
