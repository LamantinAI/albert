use rig::{
    agent::AgentBuilder, agent::NoToolConfig, completion::CompletionModel, completion::Message,
    completion::Prompt, completion::PromptError,
};

use super::{chat::chat_tools, AttemptTools};
use crate::{
    cogitator::outcome::AgentOutcome,
    cogitator::AlbertCogitator,
    models::{Failure, FailureKind, ModelSpec},
    status::StatusFeed,
};

impl AlbertCogitator {
    pub(in crate::cogitator) async fn model_attempt(
        &self,
        model: &ModelSpec,
        force_refresh: bool,
        preamble: &str,
        tools: AttemptTools,
        channel: &str,
        prompt: Message,
        history: Vec<Message>,
        feed: StatusFeed,
    ) -> Result<String, Failure> {
        match Box::pin(self.model_attempt_outcome(
            model,
            force_refresh,
            preamble,
            tools,
            channel,
            prompt,
            history,
            feed,
        ))
        .await?
        {
            AgentOutcome::Answer(answer) => Ok(answer),
            AgentOutcome::Failed(message) => Err(Failure {
                kind: FailureKind::Fatal,
                message,
            }),
            AgentOutcome::AwaitingConfirmation(_) => Err(Failure {
                kind: FailureKind::Fatal,
                message: "Confirmation is only available in an authenticated root conversation."
                    .into(),
            }),
        }
    }
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
            AttemptTools::Root(tools) => {
                self.drive(base, tools, channel, prompt, history, feed)
                    .await
            }
            AttemptTools::ChatOnly { session, pad } => {
                let stage_store = self.scratchpad.clone();
                let stage_scope = pad.clone();
                let feed =
                    feed.with_stage_source(move || stage_store.progress_scoped(&stage_scope));
                let tools = chat_tools(&self.scratchpad, pad, session);
                feed.register_tool_names(tools.iter().map(|tool| tool.name()));
                base.tools(tools)
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
