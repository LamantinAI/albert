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
    message::{AssistantContent, ReasoningContent, ToolCall, ToolFunction},
    OneOrMany,
};
use serde_json::Value;
use tracing::warn;

mod trace;
use self::trace::Trace;

/// Where a turn's status lines go, plus a durable record of the actions taken.
/// `silent()` (no target) makes every live emit a no-op, but still accumulates
/// actions — so the agent loop code stays branch-free.
#[derive(Clone)]
pub struct StatusFeed {
    feed: Option<Arc<Feed>>,
    /// The tools actually called this turn (name + clipped args + compact result).
    /// The host drains this after the loop and folds it into history, so Albert can
    /// see what he *did*, not only what he *said* (the transcript also carries replayable tool rounds).
    /// Shared across clones; survives being moved through the tool-loop.
    actions: Arc<Mutex<Vec<String>>>,
    trace: Arc<Mutex<Trace>>,
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
            feed: Some(Arc::new(Feed {
                bus,
                source,
                target,
                channel,
            })),
            actions: Arc::new(Mutex::new(Vec::new())),
            trace: Arc::new(Mutex::new(Trace::default())),
        }
    }

    /// A feed that swallows live status — for system routines and disabled config.
    /// It still records actions (harmless; the caller decides whether to use them).
    pub fn silent() -> Self {
        Self {
            feed: None,
            actions: Arc::new(Mutex::new(Vec::new())),
            trace: Arc::new(Mutex::new(Trace::default())),
        }
    }

    /// Take this turn's recorded actions, emptying the buffer.
    pub fn drain_actions(&self) -> Vec<String> {
        self.actions
            .lock()
            .map(|mut v| std::mem::take(&mut *v))
            .unwrap_or_default()
    }

    pub fn checkpoint(&self) -> Vec<Message> {
        self.trace.lock().unwrap().drain()
    }

    pub fn snapshot(&self) -> Vec<Message> {
        self.trace.lock().unwrap().clone().drain()
    }

    /// A deterministic perception call participates in the same tool journal.
    pub fn start_external(&self, id: &str, name: &str, arguments: Value) {
        let args = arguments.to_string();
        let call = ToolCall::new(
            id.into(),
            ToolFunction {
                name: name.into(),
                arguments,
            },
        );
        let mut trace = self.trace.lock().unwrap();
        trace.response(None, OneOrMany::one(AssistantContent::ToolCall(call)));
        trace.start(name, None, id, &args);
    }

    pub fn finish_external(&self, id: &str, result: &str) {
        self.trace.lock().unwrap().result(id, result);
    }

    /// Retrying a whole model attempt after tools ran could repeat external effects.
    pub fn has_tool_calls(&self) -> bool {
        self.trace.lock().unwrap().has_calls()
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
        tool_call_id: Option<String>,
        internal_call_id: &str,
        args: &str,
    ) -> impl std::future::Future<Output = ToolCallHookAction> + Send {
        self.trace.lock().unwrap().start(
            tool_name,
            tool_call_id.as_deref(),
            internal_call_id,
            args,
        );
        let feed = self.clone();
        let line = format!(
            "🔧 {tool_name} {}",
            clip(&display_args(tool_name, args).replace('\n', " "), 160)
        );
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
        internal_call_id: &str,
        args: &str,
        result: &str,
    ) -> impl std::future::Future<Output = HookAction> + Send {
        self.trace.lock().unwrap().result(internal_call_id, result);
        if let Ok(mut v) = self.actions.lock() {
            v.push(format!(
                "{tool_name} {} -> {}",
                clip(&display_args(tool_name, args).replace('\n', " "), 120),
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
        self.trace
            .lock()
            .unwrap()
            .response(response.message_id.clone(), response.choice.clone());
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

/// Tool args as shown in the live status and recorded in the action log — with secret
/// values redacted so `config_set_secret`'s value never reaches chat or history.
fn display_args(tool_name: &str, args: &str) -> String {
    if tool_name == "config_set_secret" {
        let name = serde_json::from_str::<serde_json::Value>(args)
            .ok()
            .and_then(|v| v.get("name").and_then(|n| n.as_str()).map(str::to_string))
            .unwrap_or_else(|| "?".to_string());
        return format!("{{name: {name}, value: <redacted>}}");
    }
    args.to_string()
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

#[cfg(test)]
mod rig_tests {
    //! Real rig tool-loop cancellation: one completed tool, one blocked tool, then a
    //! new user message. No provider, network or production credentials.

    use std::{
        convert::Infallible,
        future::pending,
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc, Mutex,
        },
        time::Duration,
    };

    use rig::{
        agent::AgentBuilder,
        completion::{
            CompletionError, CompletionModel, CompletionRequest, CompletionResponse, Message,
            Prompt, ToolDefinition, Usage,
        },
        message::{AssistantContent, ToolCall, ToolFunction},
        streaming::StreamingCompletionResponse,
        tool::Tool,
        OneOrMany,
    };
    use serde_json::{json, to_string, Value};
    use tokio::{spawn, sync::Notify, task::yield_now, time::timeout};

    use super::StatusFeed;

    #[derive(Clone, Default)]
    struct Model {
        round: Arc<AtomicUsize>,
        requests: Arc<Mutex<Vec<CompletionRequest>>>,
    }

    impl CompletionModel for Model {
        type Response = ();
        type StreamingResponse = ();
        type Client = Self;
        fn make(client: &Self, _: impl Into<String>) -> Self {
            client.clone()
        }
        async fn completion(
            &self,
            request: CompletionRequest,
        ) -> Result<CompletionResponse<()>, CompletionError> {
            self.requests.lock().unwrap().push(request);
            let choice = if self.round.fetch_add(1, Ordering::SeqCst) == 0 {
                OneOrMany::many([false, true].into_iter().enumerate().map(|(n, wait)| {
                    AssistantContent::ToolCall(ToolCall::new(
                        format!("call-{n}"),
                        ToolFunction {
                            name: "controlled".into(),
                            arguments: json!({"wait":wait}),
                        },
                    ))
                }))
                .unwrap()
            } else {
                OneOrMany::one(AssistantContent::text("continued"))
            };
            Ok(CompletionResponse {
                choice,
                usage: Usage::new(),
                raw_response: (),
                message_id: None,
            })
        }
        async fn stream(
            &self,
            _: CompletionRequest,
        ) -> Result<StreamingCompletionResponse<()>, CompletionError> {
            Err(CompletionError::ProviderError("unused".into()))
        }
    }

    struct Controlled {
        started: Arc<Notify>,
        dropped: Arc<AtomicBool>,
    }

    impl Tool for Controlled {
        const NAME: &'static str = "controlled";
        type Error = Infallible;
        type Args = Value;
        type Output = String;
        async fn definition(&self, _: String) -> ToolDefinition {
            ToolDefinition {
                name: Self::NAME.into(),
                description: "test".into(),
                parameters: json!({"type":"object", "properties":{"wait":{"type":"boolean"}}, "required":["wait"]}),
            }
        }
        async fn call(&self, args: Value) -> Result<String, Infallible> {
            if args["wait"] == true {
                struct OnDrop(Arc<AtomicBool>);
                impl Drop for OnDrop {
                    fn drop(&mut self) {
                        self.0.store(true, Ordering::SeqCst);
                    }
                }
                let _guard = OnDrop(self.dropped.clone());
                self.started.notify_one();
                pending::<()>().await;
            }
            Ok("completed tool data".into())
        }
    }

    #[tokio::test]
    async fn cancelled_rig_loop_resumes_with_inputs_results_and_unknown_outcome() {
        let model = Model::default();
        let started = Arc::new(Notify::new());
        let dropped = Arc::new(AtomicBool::new(false));
        let feed = StatusFeed::silent();
        let agent = AgentBuilder::new(model.clone())
            .tool(Controlled {
                started: started.clone(),
                dropped: dropped.clone(),
            })
            .build();
        let hook = feed.clone();
        let task = spawn(async move {
            agent
                .prompt("first input")
                .with_hook(hook)
                .max_turns(3)
                .await
        });
        timeout(Duration::from_secs(2), started.notified())
            .await
            .unwrap();
        timeout(Duration::from_secs(2), async {
            while !to_string(&feed.snapshot())
                .unwrap()
                .contains("completed tool data")
            {
                yield_now().await;
            }
        })
        .await
        .unwrap();
        task.abort();
        let _ = task.await;
        assert!(
            dropped.load(Ordering::SeqCst),
            "rig must drop the outstanding tool future"
        );
        let mut history = vec![Message::user("first input")];
        history.extend(feed.checkpoint());
        let agent = AgentBuilder::new(model.clone()).build();
        assert_eq!(
            agent
                .prompt("second input")
                .with_history(history)
                .await
                .unwrap(),
            "continued"
        );
        let requests = model.requests.lock().unwrap();
        let last = to_string(&requests.last().unwrap().chat_history).unwrap();
        for text in [
            "first input",
            "second input",
            "completed tool data",
            "UNKNOWN",
            "call-0",
            "call-1",
        ] {
            assert!(last.contains(text), "missing {text}: {last}");
        }
    }
}
