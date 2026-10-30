use std::sync::Arc;

use crate::{
    cogitator::AlbertCogitator, context::DiagnosticScope, messaging::contract::WorkRef,
    sessions::SessionContext, status::StatusFeed,
};
use octo_approval::Ingress;
use octo_core::{CogitatorContext, ConnectorId};
use rand::random;
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
            None,
        )
        .await
    }
}

pub(super) fn root_diagnostics(
    session: Option<&SessionContext>,
    work: Option<&WorkRef>,
    attempt: Option<&str>,
    visible: bool,
) -> DiagnosticScope {
    DiagnosticScope {
        lane: if session.is_some() || visible {
            "native"
        } else {
            "routine"
        },
        session_id: session.map(|session| session.session_id().as_str().into()),
        work_id: work.map(|work| match work {
            WorkRef::Turn(id) | WorkRef::Child(id) => id.clone(),
        }),
        attempt_id: attempt
            .map(str::to_owned)
            .unwrap_or_else(|| format!("host/{:032x}", random::<u128>())),
        run_id: None,
    }
}
