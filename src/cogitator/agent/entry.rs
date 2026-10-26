use std::sync::Arc;

use crate::{cogitator::AlbertCogitator, status::StatusFeed};
use octo_approval::Ingress;
use octo_core::{CogitatorContext, ConnectorId};
use rig::completion::Message;
impl AlbertCogitator {
    /// Build the LLM client per the configured auth mode, then run one rig
    /// tool-loop. The two modes yield different concrete model types, so the
    /// build-tools-and-run tail lives in the generic [`Self::drive`].
    pub(in crate::cogitator) async fn run_agent(
        self: &Arc<Self>,
        ctx: &CogitatorContext,
        channel: &str,
        preamble: &str,
        prompt: Message,
        history: Vec<Message>,
        reply_target: Option<ConnectorId>,
        owner: bool,
        feed: StatusFeed,
        scope: Option<&str>,
    ) -> (String, Option<String>) {
        self.run_agent_with_ingress(
            ctx,
            channel,
            preamble,
            prompt,
            history,
            reply_target,
            owner,
            feed,
            scope,
            None,
        )
        .await
    }

    pub(in crate::cogitator) async fn run_agent_with_ingress(
        self: &Arc<Self>,
        ctx: &CogitatorContext,
        channel: &str,
        preamble: &str,
        prompt: Message,
        history: Vec<Message>,
        reply_target: Option<ConnectorId>,
        owner: bool,
        feed: StatusFeed,
        scope: Option<&str>,
        ingress: Option<Ingress>,
    ) -> (String, Option<String>) {
        self.run_agent_scoped(
            ctx,
            channel,
            preamble,
            prompt,
            history,
            reply_target,
            owner,
            feed,
            scope,
            ingress,
            None,
        )
        .await
    }
}
