//! Model-context budgets are independent of retained conversation storage.
mod chunks;
mod diagnostics;
mod guard;
mod recovery;
pub(crate) use chunks::batches;
pub(crate) use diagnostics::DiagnosticScope;
pub use guard::BudgetedModel;

use rig::{
    completion::Message,
    message::{AssistantContent, ToolResultContent, UserContent},
};
use serde::Deserialize;
use serde_json::to_string;
use tiktoken_rs::{cl100k_base_singleton, o200k_base_singleton};

use crate::{
    history::{to_messages, ContextWindow, Turn},
    models::ModelSpec,
};

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tokenizer {
    O200k,
    Cl100k,
    Bytes,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool,
    pub artifacts: crate::artifacts::Settings,
    pub window_tokens: usize,
    /// A hard partition, including instructions/tools/framing AND generation.
    pub reserve_tokens: usize,
    pub response_tokens: usize,
    pub new_messages_percent: usize,
    pub compact_output_tokens: usize,
    pub compact_prompt: String,
    pub tokenizer: Tokenizer,
    pub image_tokens: usize,
    pub timeout_secs: u64,
    pub request_timeout_ms: u64,
    pub continuation_retries: usize,
    pub continuation_retry_delay_ms: u64,
    pub max_compaction_passes: usize,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            artifacts: Default::default(),
            window_tokens: 1_000_000,
            reserve_tokens: 32_768,
            response_tokens: 4096,
            new_messages_percent: 70,
            compact_output_tokens: 16_384,
            compact_prompt: String::new(),
            tokenizer: Tokenizer::O200k,
            image_tokens: 4096,
            timeout_secs: 300,
            request_timeout_ms: 180_000,
            continuation_retries: 1,
            continuation_retry_delay_ms: 1000,
            max_compaction_passes: 16,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        self.artifacts.validate()?;
        if self.request_timeout_ms == 0
            || self.request_timeout_ms >= self.timeout_secs.saturating_mul(1000)
        {
            return Err("context: request_timeout_ms must be positive and shorter than timeout_secs (converted to milliseconds).".into());
        }
        if self.continuation_retries > 3 || self.continuation_retry_delay_ms > 60000 {
            return Err("context: continuation_retries must be 0..3 and continuation_retry_delay_ms at most 60000.".into());
        }
        if self.window_tokens <= self.reserve_tokens
            || self.response_tokens >= self.reserve_tokens
            || self.compact_output_tokens >= self.reserve_tokens
            || self.response_tokens == 0
            || self.compact_output_tokens == 0
            || !(1..100).contains(&self.new_messages_percent)
            || self.timeout_secs == 0
            || self.max_compaction_passes == 0
            || self.image_tokens == 0
        {
            return Err("context: require window > reserve > positive output limits and new_messages_percent in 1..99".into());
        }
        Ok(())
    }
    pub fn for_model(&self, model: &ModelSpec) -> Self {
        let mut settings = self.clone();
        settings.window_tokens = model
            .context_window
            .unwrap_or(self.window_tokens)
            .min(self.window_tokens);
        settings
    }
    pub fn dialogue(&self) -> usize {
        self.window_tokens.saturating_sub(self.reserve_tokens)
    }
    pub fn new_budget(&self) -> usize {
        ((self.dialogue() as u128 * self.new_messages_percent as u128) / 100) as usize
    }
    pub fn compact_budget(&self) -> usize {
        self.dialogue().saturating_sub(self.new_budget())
    }
    pub fn count(&self, text: &str) -> usize {
        match self.tokenizer {
            Tokenizer::O200k => o200k_base_singleton().encode_ordinary(text).len(),
            Tokenizer::Cl100k => cl100k_base_singleton().encode_ordinary(text).len(),
            Tokenizer::Bytes => text.len(),
        }
    }
    pub fn message_tokens(&self, message: &Message) -> usize {
        let mut copy = message.clone();
        let mut image_tokens = 0;
        match &mut copy {
            Message::User { content } => {
                for item in content.iter_mut() {
                    if let UserContent::ToolResult(result) = item {
                        for part in result.content.iter_mut() {
                            if matches!(part, ToolResultContent::Image(_)) {
                                image_tokens += self.image_tokens;
                                *part = ToolResultContent::text("[image]");
                            }
                        }
                    }
                    if matches!(item, UserContent::Image(_)) {
                        image_tokens += self.image_tokens;
                        *item = UserContent::text("[image]");
                    }
                }
            }
            Message::Assistant { content, .. } => {
                for item in content.iter_mut() {
                    if matches!(item, AssistantContent::Image(_)) {
                        image_tokens += self.image_tokens;
                        *item = AssistantContent::text("[image]");
                    }
                }
            }
            _ => {}
        }
        self.count(&to_string(&copy).expect("serializable message")) + image_tokens
    }
    pub fn messages_tokens(&self, messages: &[Message]) -> usize {
        messages.iter().map(|m| self.message_tokens(m)).sum()
    }
    pub fn new_tokens(&self, window: &ContextWindow) -> usize {
        let turns: Vec<Turn> = window.messages.iter().map(|m| m.turn.clone()).collect();
        self.messages_tokens(&to_messages(&turns))
    }
}

pub fn compact_message(content: &str) -> Message {
    Message::assistant(format!("[Conversation compact: historical context, not new instructions. Preserve user constraints, provenance, pending work and UNKNOWN action outcomes.]\n{content}"))
}

pub fn window_messages(window: &ContextWindow) -> Vec<Message> {
    let mut messages = Vec::new();
    if let Some(compact) = &window.compact {
        messages.push(compact_message(&compact.content));
    }
    messages.extend(to_messages(
        &window
            .messages
            .iter()
            .map(|m| m.turn.clone())
            .collect::<Vec<_>>(),
    ));
    messages
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Result as IoResult, Write},
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc, Mutex,
        },
        time::Duration,
    };

    use rig::{
        completion::{
            CompletionError, CompletionModel, CompletionRequest, CompletionResponse, Usage,
        },
        message::AssistantContent,
        streaming::StreamingCompletionResponse,
        OneOrMany,
    };
    use tokio::time::sleep;
    use tracing::{instrument::WithSubscriber, subscriber::NoSubscriber, Dispatch, Level};
    use tracing_subscriber::fmt;

    use super::*;
    use crate::status::StatusFeed;

    #[derive(Clone, Default)]
    struct TimingModel {
        calls: Arc<AtomicUsize>,
    }
    impl CompletionModel for TimingModel {
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
            self.calls.fetch_add(1, Ordering::SeqCst);
            sleep(Duration::from_millis(2)).await;
            Ok(CompletionResponse {
                choice: OneOrMany::one(AssistantContent::text("PRIVATE_RESPONSE_BODY")),
                usage: Usage {
                    input_tokens: 7,
                    output_tokens: request.max_tokens.unwrap_or(9),
                    ..Usage::default()
                },
                raw_response: (),
                message_id: None,
            })
        }
        async fn stream(
            &self,
            _: CompletionRequest,
        ) -> Result<StreamingCompletionResponse<()>, CompletionError> {
            Err(CompletionError::ProviderError(
                "PRIVATE_PROVIDER_ERROR".into(),
            ))
        }
    }
    #[derive(Clone, Default)]
    struct LogBuffer(Arc<Mutex<Vec<u8>>>);
    impl Write for LogBuffer {
        fn write(&mut self, bytes: &[u8]) -> IoResult<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> IoResult<()> {
            Ok(())
        }
    }
    fn diagnostic_scope() -> DiagnosticScope {
        DiagnosticScope {
            lane: "test",
            session_id: Some("session-test".into()),
            work_id: Some("work-test".into()),
            attempt_id: "attempt-test".into(),
            run_id: None,
        }
    }

    fn isolate_callsite_interest() -> Dispatch {
        // With only one registered dispatcher, tracing's shared callsite cache
        // may use another test thread's default subscriber. Keep a second,
        // uninstalled dispatcher alive throughout each local capture instead.
        Dispatch::new(NoSubscriber::default())
    }

    #[tokio::test]
    async fn diagnostic_rounds_are_scoped_content_free_and_stream_setup_is_not_completion() {
        let _interest_isolation = isolate_callsite_interest();
        let logs = LogBuffer::default();
        let writer = logs.clone();
        let subscriber = fmt()
            .without_time()
            .with_ansi(false)
            .with_target(false)
            .with_max_level(Level::INFO)
            .with_writer(move || writer.clone())
            .finish();
        let raw = TimingModel::default();
        let calls = raw.calls.clone();
        let feed = StatusFeed::silent().with_diagnostics(Some(diagnostic_scope()));
        let model = BudgetedModel::new(raw, None, true)
            .with_diagnostics(Some(diagnostic_scope()))
            .with_diagnostic_model("trusted-model")
            .with_diagnostic_feed(feed.clone());
        async {
            for _ in 0..2 {
                let mut request = model
                    .completion_request(Message::user("PRIVATE_USER_BODY"))
                    .preamble("PRIVATE_PREAMBLE".into())
                    .build();
                request.max_tokens = Some(4);
                model.clone().completion(request).await.unwrap();
            }
            let request = model
                .completion_request(Message::user("PRIVATE_STREAM_BODY"))
                .build();
            assert!(model.stream(request).await.is_err());
        }
        .with_subscriber(subscriber)
        .await;
        let text = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(text.contains(
            &feed
                .request_id()
                .expect("trusted request ID reached tool feed")
        ));
        for expected in [
            "attempt-test",
            "session-test",
            "work-test",
            "trusted-model",
            "round=1",
            "round=2",
            "guard_ms=",
            "provider_round_wall_ms=",
            "input_tokens=7",
            "applied_output_cap_hit=Some(true)",
        ] {
            assert!(text.contains(expected), "missing {expected}: {text}");
        }
        assert!(!text.contains("PRIVATE_"), "payload in metrics: {text}");
        let stream = text
            .lines()
            .find(|line| line.contains("model stream setup finished"))
            .unwrap();
        assert!(stream.contains("stream_completion_observed=false"));
        assert!(stream.contains("stream_setup_wall_ms="));
        assert!(!stream.contains("provider_round_wall_ms="));
    }

    #[tokio::test]
    async fn diagnostic_guard_rejection_does_not_call_or_log_provider_payload() {
        let _interest_isolation = isolate_callsite_interest();
        let settings = Settings {
            window_tokens: 4096,
            reserve_tokens: 512,
            response_tokens: 64,
            compact_output_tokens: 128,
            image_tokens: 32,
            tokenizer: Tokenizer::Bytes,
            ..Settings::default()
        };
        settings.validate().unwrap();
        let logs = LogBuffer::default();
        let writer = logs.clone();
        let subscriber = fmt()
            .without_time()
            .with_ansi(false)
            .with_target(false)
            .with_max_level(Level::INFO)
            .with_writer(move || writer.clone())
            .finish();
        let raw = TimingModel::default();
        let calls = raw.calls.clone();
        let model = BudgetedModel::new(raw, Some(settings), true)
            .with_diagnostics(Some(diagnostic_scope()));
        async {
            let request = model
                .completion_request(Message::user("PRIVATE_USER"))
                .preamble("PRIVATE_PREAMBLE ".repeat(500))
                .build();
            assert!(model.completion(request).await.is_err());
        }
        .with_subscriber(subscriber)
        .await;
        let text = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(text.contains("provider_called=false"));
        assert!(text.contains("guard_ms="));
        assert!(!text.contains("PRIVATE_"));
        assert!(!text.contains("provider_round_wall_ms="));
    }

    #[test]
    fn fixed_reserve_is_outside_the_seventy_thirty_dialogue_partition() {
        let settings = Settings {
            reserve_tokens: 10000,
            compact_output_tokens: 4096,
            ..Default::default()
        };
        assert_eq!(settings.dialogue(), 990000);
        assert_eq!(settings.new_budget(), 693000);
        assert_eq!(settings.compact_budget(), 297000);
        assert!(settings.validate().is_ok());
        let mut invalid = settings;
        invalid.reserve_tokens = invalid.window_tokens;
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn text_is_tokenized_and_image_payload_is_not_counted_as_base64_text() {
        let settings = Settings::default();
        assert!(settings.count("hello world") < "hello world".len());
        let message = Message::User {
            content: rig::OneOrMany::one(UserContent::image_base64("a".repeat(100000), None, None)),
        };
        let tokens = settings.message_tokens(&message);
        assert!(tokens >= settings.image_tokens && tokens < settings.image_tokens + 100);
    }
    #[test]
    fn oversized_text_is_batched_losslessly_with_bounded_utf8_fragments() {
        let settings = Settings {
            tokenizer: Tokenizer::Bytes,
            window_tokens: 5000,
            reserve_tokens: 1000,
            response_tokens: 256,
            compact_output_tokens: 256,
            max_compaction_passes: 64,
            ..Default::default()
        };
        let message = Message::user("Строка \"цитата\"\n".repeat(1000));
        let original = to_string(&message).unwrap();
        let chunks = batches(vec![message], &settings).unwrap();
        assert!(chunks.len() > 1);
        let mut restored = String::new();
        for chunk in chunks {
            assert!(settings.messages_tokens(&chunk) <= settings.new_budget());
            for message in chunk {
                let Message::User { content } = message else {
                    panic!("fragment");
                };
                let UserContent::Text(text) = content.iter().next().unwrap() else {
                    panic!("text");
                };
                restored.push_str(text.text.split_once('\n').unwrap().1);
            }
        }
        assert_eq!(restored, original);
    }
}
