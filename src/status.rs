//! `StatusFeed` — a rig [`PromptHook`] that streams the agent's live progress
//! into the chat while a turn runs: each tool call (and, when the provider
//! returns them, reasoning summaries) becomes a `chat.status` envelope aimed at
//! the source connector. The Telegram connector renders those as one in-place
//! edited italic status message per turn (openclaw-style); the console just
//! prints them.
//!
//! The feed is deliberately fire-and-forget: a failed publish is logged and the
//! turn goes on — live feedback must never break cognition.

use std::sync::Arc;

use octo_core::{ChannelId, ConnectorId, Envelope, EventBus as _, EventKind, InProcessBus};
use rig::{
    agent::{HookAction, PromptHook, ToolCallHookAction},
    completion::{CompletionModel, CompletionResponse, Message},
    message::{AssistantContent, ReasoningContent},
};
use tracing::warn;

/// Where a turn's status lines go. `silent()` (no target) makes every emit a
/// no-op, so the agent loop code stays branch-free.
#[derive(Clone)]
pub struct StatusFeed(Option<Arc<Feed>>);

struct Feed {
    bus: Arc<InProcessBus>,
    source: ConnectorId,
    target: ConnectorId,
    channel: Option<ChannelId>,
}

impl StatusFeed {
    pub fn new(
        bus: Arc<InProcessBus>,
        source: ConnectorId,
        target: ConnectorId,
        channel: Option<ChannelId>,
    ) -> Self {
        Self(Some(Arc::new(Feed { bus, source, target, channel })))
    }

    /// A feed that swallows everything — for system routines and disabled config.
    pub fn silent() -> Self {
        Self(None)
    }

    async fn emit(&self, line: String) {
        let Some(feed) = &self.0 else { return };
        let mut env = Envelope::new(
            feed.source.clone(),
            EventKind::from_static("chat.status"),
            line,
        )
        .with_target(feed.target.clone());
        if let Some(ch) = &feed.channel {
            env = env.with_channel(ch.clone());
        }
        if let Err(e) = feed.bus.publish(env).await {
            warn!(error = %e, "failed to publish chat.status");
        }
    }
}

impl<M: CompletionModel> PromptHook<M> for StatusFeed {
    /// Before each tool runs: show which one, with a clipped arg preview.
    fn on_tool_call(
        &self,
        tool_name: &str,
        _tool_call_id: Option<String>,
        _internal_call_id: &str,
        args: &str,
    ) -> impl std::future::Future<Output = ToolCallHookAction> + Send {
        let feed = self.clone();
        let line = format!("🔧 {tool_name} {}", clip(&args.replace('\n', " "), 160));
        async move {
            feed.emit(line).await;
            ToolCallHookAction::cont()
        }
    }

    /// After each model round: surface any reasoning the provider returned
    /// (e.g. Codex reasoning summaries) as the agent's "thoughts".
    fn on_completion_response(
        &self,
        _prompt: &Message,
        response: &CompletionResponse<M::Response>,
    ) -> impl std::future::Future<Output = HookAction> + Send {
        let feed = self.clone();
        let thoughts: Vec<String> = response
            .choice
            .iter()
            .filter_map(|c| match c {
                AssistantContent::Reasoning(r) => {
                    let text = r
                        .content
                        .iter()
                        .filter_map(|rc| match rc {
                            ReasoningContent::Text { text, .. } => Some(text.as_str()),
                            ReasoningContent::Summary(text) => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    (!text.trim().is_empty()).then(|| format!("💭 {}", clip(&text, 280)))
                }
                _ => None,
            })
            .collect();
        async move {
            for line in thoughts {
                feed.emit(line).await;
            }
            HookAction::cont()
        }
    }
}

/// Clip to at most `max` chars on a char boundary, marking the cut with an ellipsis.
fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    format!("{cut}…")
}
