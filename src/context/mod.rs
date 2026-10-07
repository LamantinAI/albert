//! Model-context budgets are independent of retained conversation storage.
mod chunks;
mod guard;
mod recovery;
pub(crate) use chunks::batches;
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
    use super::*;

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
