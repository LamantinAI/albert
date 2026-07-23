//! `StatusFeed` — a rig [`PromptHook`] that streams the agent's live progress
//! into the chat while a turn runs: each tool call (and, when the provider
//! returns them, reasoning summaries) becomes a `chat.status` envelope aimed at
//! the source connector. The Telegram connector renders those as one in-place
//! edited italic status message per turn (openclaw-style); the console just
//! prints them.
//!
//! The feed is deliberately fire-and-forget: a failed publish is logged and the
//! turn goes on — live feedback must never break cognition.

use std::sync::{Arc, Mutex};

use octo_core::{ChannelId, ConnectorId, Envelope, EventBus as _, EventKind, InProcessBus};
use rig::{
    agent::{HookAction, PromptHook, ToolCallHookAction},
    completion::{CompletionModel, CompletionResponse, Message},
    message::{AssistantContent, ReasoningContent},
};
use tracing::warn;

/// Where a turn's status lines go, plus a durable record of the actions taken.
/// `silent()` (no target) makes every live emit a no-op, but still accumulates
/// actions — so the agent loop code stays branch-free.
#[derive(Clone)]
pub struct StatusFeed {
    feed: Option<Arc<Feed>>,
    /// The tools actually called this turn (name + clipped args + compact result).
    /// The host drains this after the loop and folds it into history, so Albert can
    /// see what he *did*, not only what he *said* (the transcript keeps only text).
    /// Shared across clones; survives being moved through the tool-loop.
    actions: Arc<Mutex<Vec<String>>>,
}

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
        Self {
            feed: Some(Arc::new(Feed { bus, source, target, channel })),
            actions: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// A feed that swallows live status — for system routines and disabled config.
    /// It still records actions (harmless; the caller decides whether to use them).
    pub fn silent() -> Self {
        Self { feed: None, actions: Arc::new(Mutex::new(Vec::new())) }
    }

    /// Take this turn's recorded actions, emptying the buffer.
    pub fn drain_actions(&self) -> Vec<String> {
        self.actions
            .lock()
            .map(|mut v| std::mem::take(&mut *v))
            .unwrap_or_default()
    }

    async fn emit(&self, line: String) {
        let Some(feed) = &self.feed else { return };
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

    /// After each tool returns: record a compact, durable line of what was done —
    /// name + (kept) arguments + a hard-compressed result. Args are cheap and are the
    /// "what did I do"; results are the expensive half, so they collapse to ok/err.
    fn on_tool_result(
        &self,
        tool_name: &str,
        _tool_call_id: Option<String>,
        _internal_call_id: &str,
        args: &str,
        result: &str,
    ) -> impl std::future::Future<Output = HookAction> + Send {
        if let Ok(mut v) = self.actions.lock() {
            v.push(format!(
                "{tool_name} {} -> {}",
                clip(&args.replace('\n', " "), 120),
                summarize_result(result),
            ));
        }
        async { HookAction::cont() }
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

/// Compress a tool result to a few tokens — `ok` / `err: …` when the payload says so,
/// else a hard clip. Results are the expensive half of an action record; args are kept.
fn summarize_result(result: &str) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(result) {
        if let Some(e) = v.get("error").and_then(|e| e.as_str()) {
            return format!("err: {}", clip(e, 80));
        }
        match v.get("ok").and_then(serde_json::Value::as_bool) {
            Some(true) => return "ok".to_string(),
            Some(false) => return "err".to_string(),
            None => {}
        }
    }
    clip(&result.replace('\n', " "), 80)
}

/// Clip to at most `max` chars on a char boundary, marking the cut with an ellipsis.
fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    format!("{cut}…")
}
