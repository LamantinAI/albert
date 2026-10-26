//! `StatusFeed` — a rig [`PromptHook`] that streams the agent's live progress
//! into the chat while a turn runs: each tool call (and, when the provider
//! returns them, reasoning summaries) becomes a `chat.status` envelope aimed at
//! the source connector. The Telegram connector renders those as one in-place
//! edited italic status message per turn (openclaw-style); the console just
//! prints them.
//!
//! The feed is deliberately fire-and-forget: a failed publish is logged and the
//! turn goes on — live feedback must never break cognition.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use octo_connector_telegram::TurnFeedback;
use octo_core::{ChannelId, ConnectorId, Envelope, EventBus as _, EventKind, InProcessBus};
use rig::{
    completion::Message,
    message::{AssistantContent, ToolCall, ToolFunction},
    OneOrMany,
};
use serde_json::Value;
use tokio::sync::RwLock;
use tracing::warn;

use crate::history::sessions::Backend;
use crate::sessions::{telegram::output_scope, SessionContext};

mod feedback;
mod hooks;
mod trace;
use self::trace::Trace;

/// The Responses-API identity for a host-made tool call: a `fc_…` item id and a
/// `call_…` call id, both derived from the host's own id. Used when journaling an
/// external call and when repairing a stored round that was journaled without them.
pub fn responses_ids(id: &str) -> (String, String) {
    let slug: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    (format!("fc_{slug}"), format!("call_{slug}"))
}

/// Where a turn's status lines go, plus a durable record of the actions taken.
/// `silent()` (no target) makes every live emit a no-op, but still accumulates
/// actions — so the agent loop code stays branch-free.
type ResultObserver = dyn Fn(&str, &str, &str) + Send + Sync;

#[derive(Clone)]
pub struct StatusFeed {
    feed: Option<Arc<Feed>>,
    /// The tools actually called this turn (name + clipped args + compact result).
    /// The host drains this after the loop and folds it into history, so Albert can
    /// see what he *did*, not only what he *said* (the transcript also carries replayable tool rounds).
    /// Shared across clones; survives being moved through the tool-loop.
    actions: Arc<Mutex<Vec<String>>>,
    trace: Arc<Mutex<Trace>>,
    result_observer: Option<Arc<ResultObserver>>,
    session_context: Option<SessionContext>,
    discarded: Arc<AtomicBool>,
    attempt_id: Option<String>,
    feedback: Option<TurnFeedback>,
}

#[derive(Clone)]
struct Feed {
    bus: Arc<InProcessBus>,
    source: ConnectorId,
    target: ConnectorId,
    channel: Option<ChannelId>,
    session: Option<String>,
    backend: Option<Arc<RwLock<Backend>>>,
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
                session: None,
                backend: None,
            })),
            actions: Arc::new(Mutex::new(Vec::new())),
            trace: Arc::new(Mutex::new(Trace::default())),
            result_observer: None,
            session_context: None,
            discarded: Arc::new(AtomicBool::new(false)),
            attempt_id: None,
            feedback: None,
        }
    }

    pub(crate) fn with_session(mut self, session: Option<&str>) -> Self {
        if let Some(feed) = self.feed.as_mut() {
            Arc::make_mut(feed).session = session.map(str::to_owned);
        }
        self
    }

    pub(crate) fn with_backend(mut self, backend: Arc<RwLock<Backend>>) -> Self {
        if let Some(feed) = self.feed.as_mut() {
            Arc::make_mut(feed).backend = Some(backend);
        }
        self
    }

    /// A feed that swallows live status — for system routines and disabled config.
    /// It still records actions (harmless; the caller decides whether to use them).
    pub fn silent() -> Self {
        Self {
            feed: None,
            actions: Arc::new(Mutex::new(Vec::new())),
            trace: Arc::new(Mutex::new(Trace::default())),
            result_observer: None,
            session_context: None,
            discarded: Arc::new(AtomicBool::new(false)),
            attempt_id: None,
            feedback: None,
        }
    }

    pub(crate) fn with_session_context(mut self, context: Option<SessionContext>) -> Self {
        self.session_context = context;
        self
    }
    pub(crate) fn with_attempt(mut self, attempt_id: String) -> Self {
        self.attempt_id = Some(attempt_id);
        self
    }

    pub(crate) fn is_live(&self) -> bool {
        !self.discarded.load(Ordering::Acquire)
            && self
                .session_context
                .as_ref()
                .is_none_or(SessionContext::is_live)
    }
    pub(crate) fn fenced_silent(&self) -> Self {
        let mut feed = Self::silent();
        feed.session_context = self.session_context.clone();
        feed.discarded = self.discarded.clone();
        feed
    }

    pub(crate) fn discard(&self) -> Result<(), &'static str> {
        self.discarded.store(true, Ordering::Release);
        let mut poisoned = false;
        let mut trace = self.trace.lock().unwrap_or_else(|error| {
            poisoned = true;
            error.into_inner()
        });
        *trace = Trace::default();
        drop(trace);
        self.actions
            .lock()
            .unwrap_or_else(|error| {
                poisoned = true;
                error.into_inner()
            })
            .clear();
        if poisoned {
            Err("status buffers were poisoned during cleanup")
        } else {
            Ok(())
        }
    }

    /// Called only after the tool response has reached the parent and its trace.
    pub fn with_result_observer(
        mut self,
        observer: impl Fn(&str, &str, &str) + Send + Sync + 'static,
    ) -> Self {
        self.result_observer = Some(Arc::new(observer));
        self
    }

    /// Take this turn's recorded actions, emptying the buffer.
    pub fn drain_actions(&self) -> Vec<String> {
        self.actions
            .lock()
            .map(|mut v| std::mem::take(&mut *v))
            .unwrap_or_default()
    }

    pub fn checkpoint(&self) -> Vec<Message> {
        let mut trace = self.trace.lock().unwrap();
        if self.is_live() {
            trace.drain()
        } else {
            Vec::new()
        }
    }

    pub fn snapshot(&self) -> Vec<Message> {
        let trace = self.trace.lock().unwrap();
        if self.is_live() {
            trace.clone().drain()
        } else {
            Vec::new()
        }
    }

    /// A deterministic perception call participates in the same tool journal.
    ///
    /// The journal is replayed to the model as an ordinary tool round, so the call
    /// must look like one the model made: the OpenAI Responses API refuses a
    /// function call without a `call_id` (rig fails the whole request before it is
    /// sent), and names a function-call item `fc_…`.
    pub fn start_external(&self, id: &str, name: &str, arguments: Value) {
        let args = arguments.to_string();
        let (item_id, call_id) = responses_ids(id);
        let call = ToolCall::new(
            item_id,
            ToolFunction {
                name: name.into(),
                arguments,
            },
        )
        .with_call_id(call_id.clone());
        let mut trace = self.trace.lock().unwrap();
        if !self.is_live() {
            return;
        }
        trace.response(None, OneOrMany::one(AssistantContent::ToolCall(call)));
        trace.start(name, Some(&call_id), id, &args);
    }

    pub fn finish_external(&self, id: &str, result: &str) {
        let mut trace = self.trace.lock().unwrap();
        if self.is_live() {
            trace.result(id, result);
        }
    }

    /// Retrying a whole model attempt after tools ran could repeat external effects.
    pub fn tool_call_count(&self) -> usize {
        self.trace.lock().unwrap().call_count()
    }

    /// Host operation progress is ephemeral: never append it to the conversation
    /// or execution journal, and never expose provider reasoning/model names here.
    pub(crate) async fn progress(&self, line: impl Into<String>) {
        self.emit(line.into()).await;
    }

    async fn emit(&self, line: String) {
        if !self.is_live() || !self.feedback_open() {
            return;
        }
        let Some(feed) = &self.feed else { return };
        let lease = match &feed.backend {
            Some(backend) => Some(backend.read().await),
            None => None,
        };
        if lease
            .as_ref()
            .is_some_and(|backend| **backend == Backend::Closed)
        {
            return;
        }
        let mut env = Envelope::new(
            feed.source.clone(),
            EventKind::from_static("chat.status"),
            line,
        )
        .with_target(feed.target.clone());
        if let Some(ch) = &feed.channel {
            env = env.with_channel(ch.clone());
        }
        if let Some(session) = &feed.session {
            env = env.with_tag("session_id", session);
        }
        if let Some(session) = &self.session_context {
            env = env
                .with_context(session.clone())
                .with_context(output_scope(session, self.attempt_id.clone()));
        }
        if let Some(feedback) = &self.feedback {
            env = env.with_context(feedback.clone());
        }
        if let Err(e) = feed.bus.publish(env).await {
            warn!(error = %e, "failed to publish chat.status");
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
        providers::openai::responses_api::InputItem,
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

    #[test]
    fn external_call_replays_as_a_valid_responses_round() {
        let feed = StatusFeed::silent();
        let id = "auto-hear-225894988:1791189153035";
        feed.start_external(id, "dispatch_to_connector", json!({"target":"transcribe"}));
        feed.finish_external(id, "{\"text\":\"hello\"}");
        let history = feed.snapshot();
        assert_eq!(history.len(), 2);
        let mut items = Vec::new();
        for message in history {
            items.extend(Vec::<InputItem>::try_from(message).expect("Responses-legal round"));
        }
        let wire = to_string(&items).unwrap();
        assert!(
            wire.contains("\"call_id\":\"call_auto_hear_225894988_1791189153035\""),
            "{wire}"
        );
        assert!(
            wire.contains("\"id\":\"fc_auto_hear_225894988_1791189153035\""),
            "{wire}"
        );
        assert!(
            wire.contains("hello"),
            "the transcript is the tool output: {wire}"
        );
    }
    #[test]
    fn interrupted_hearing_keeps_responses_pair() {
        let feed = StatusFeed::silent();
        feed.start_external(
            "auto-hear-1:2",
            "dispatch_to_connector",
            json!({"target":"transcribe"}),
        );
        let mut items = Vec::new();
        for message in feed.checkpoint() {
            items.extend(Vec::<InputItem>::try_from(message).unwrap());
        }
        let wire = to_string(&items).unwrap();
        assert_eq!(
            wire.matches("\"call_id\":\"call_auto_hear_1_2\"").count(),
            2,
            "{wire}"
        );
        assert!(wire.contains("UNKNOWN"), "{wire}");
    }
}
