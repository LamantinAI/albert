//! Replayable tool rounds, including an explicit result for interrupted calls.
//! Kept independently of the model future so dropping that future loses no work.

use rig::{
    completion::Message,
    message::{AssistantContent, ToolCall, ToolResultContent, UserContent},
    OneOrMany,
};
use serde_json::{from_str, Value};

#[derive(Clone, Default)]
pub(super) struct Trace {
    rounds: Vec<Round>,
}

#[derive(Clone)]
struct Round {
    assistant: Message,
    calls: Vec<Call>,
}

#[derive(Clone)]
struct Call {
    original: ToolCall,
    internal_id: Option<String>,
    result: Option<String>,
}

impl Trace {
    pub fn response(&mut self, id: Option<String>, content: OneOrMany<AssistantContent>) {
        let calls: Vec<_> = content
            .iter()
            .filter_map(|item| match item {
                AssistantContent::ToolCall(call) => Some(Call {
                    original: call.clone(),
                    internal_id: None,
                    result: None,
                }),
                _ => None,
            })
            .collect();
        if calls.is_empty() {
            return;
        } // Unsent final text is not a delivered answer.
        let content = content.into_iter().map(|mut item| {
            if let AssistantContent::ToolCall(call) = &mut item {
                if call.function.name == "config_set_secret" {
                    if let Some(args) = call.function.arguments.as_object_mut() {
                        args.insert("value".into(), Value::String("<redacted>".into()));
                    } else {
                        call.function.arguments = Value::String("<redacted>".into());
                    }
                }
            }
            item
        });
        self.rounds.push(Round {
            assistant: Message::Assistant {
                id,
                content: OneOrMany::many(content).expect("nonempty response"),
            },
            calls,
        });
    }

    pub fn start(&mut self, name: &str, call_id: Option<&str>, internal_id: &str, args: &str) {
        let args = from_str::<Value>(args).ok();
        if let Some(call) = self.rounds.last_mut().and_then(|round| {
            round.calls.iter_mut().find(|c| {
                c.internal_id.is_none()
                    && c.original.function.name == name
                    && c.original.call_id.as_deref() == call_id
                    && args.as_ref() == Some(&c.original.function.arguments)
            })
        }) {
            call.internal_id = Some(internal_id.to_string());
        }
    }

    pub fn result(&mut self, internal_id: &str, result: &str) {
        if let Some(call) = self
            .rounds
            .iter_mut()
            .rev()
            .flat_map(|r| r.calls.iter_mut())
            .find(|c| c.internal_id.as_deref() == Some(internal_id))
        {
            call.result = Some(result.to_string());
        }
    }

    pub fn call_count(&self) -> usize {
        self.rounds.iter().map(|round| round.calls.len()).sum()
    }

    pub fn drain(&mut self) -> Vec<Message> {
        let mut messages = Vec::new();
        for round in self.rounds.drain(..) {
            messages.push(round.assistant);
            for call in round.calls {
                let result = call.result.unwrap_or_else(|| {
                    if call.internal_id.is_some() {
                        "[Interrupted while awaiting this tool. Its outcome is UNKNOWN: external effects may already have happened. Do not blindly repeat it; verify state first.]".into()
                    } else {
                        "[Interrupted before this tool was invoked. It was NOT executed.]".into()
                    }
                });
                let content = ToolResultContent::from_tool_output(result);
                let result = match call.original.call_id {
                    Some(call_id) => {
                        UserContent::tool_result_with_call_id(call.original.id, call_id, content)
                    }
                    None => UserContent::tool_result(call.original.id, content),
                };
                messages.push(Message::User {
                    content: OneOrMany::one(result),
                });
            }
        }
        messages
    }
}

#[cfg(test)]
mod tests {
    use super::Trace;
    use rig::{
        completion::Message,
        message::{AssistantContent, ToolCall, ToolFunction},
        OneOrMany,
    };
    use serde_json::{json, to_string};

    fn call(id: &str, name: &str) -> ToolCall {
        ToolCall::new(
            id.into(),
            ToolFunction {
                name: name.into(),
                arguments: json!({"path":"report.txt"}),
            },
        )
    }

    #[test]
    fn interruption_retains_results_and_closes_every_outstanding_call() {
        let mut trace = Trace::default();
        trace.response(
            None,
            OneOrMany::many([
                AssistantContent::ToolCall(call("a", "read")),
                AssistantContent::ToolCall(call("b", "write")),
                AssistantContent::ToolCall(call("c", "search")),
            ])
            .unwrap(),
        );
        trace.start("read", None, "internal-a", r#"{"path":"report.txt"}"#);
        trace.result("internal-a", "The complete result, not a clipped summary");
        trace.start("write", None, "internal-b", r#"{"path":"report.txt"}"#);
        let messages = trace.drain();
        assert_eq!(messages.len(), 4);
        let json = to_string(&messages).unwrap();
        assert!(json.contains("The complete result, not a clipped summary"));
        assert!(json.contains("UNKNOWN"));
        assert!(json.contains("NOT executed"));
        assert!(trace.drain().is_empty());
    }

    #[test]
    fn identical_parallel_calls_have_distinct_results_and_codex_ids() {
        let mut trace = Trace::default();
        let mut a = call("item-a", "read");
        a.call_id = Some("call-a".into());
        let mut b = a.clone();
        b.id = "item-b".into();
        b.call_id = Some("call-b".into());
        trace.response(
            Some("response-id".into()),
            OneOrMany::many([AssistantContent::ToolCall(a), AssistantContent::ToolCall(b)])
                .unwrap(),
        );
        trace.start("read", Some("call-b"), "b", r#"{"path":"report.txt"}"#);
        trace.start("read", Some("call-a"), "a", r#"{"path":"report.txt"}"#);
        trace.result("b", "second");
        trace.result("a", "first");
        let messages = trace.drain();
        assert_eq!(
            messages[1],
            Message::tool_result_with_call_id("item-a", Some("call-a".into()), "first")
        );
        assert_eq!(
            messages[2],
            Message::tool_result_with_call_id("item-b", Some("call-b".into()), "second")
        );
    }

    #[test]
    fn unsent_final_answers_are_not_replayed_and_secrets_are_redacted() {
        let mut trace = Trace::default();
        trace.response(
            None,
            OneOrMany::one(AssistantContent::text("Unsent answer")),
        );
        assert!(trace.drain().is_empty());
        let secret = ToolCall::new(
            "x".into(),
            ToolFunction {
                name: "config_set_secret".into(),
                arguments: json!({"name":"KEY", "value":"sensitive"}),
            },
        );
        trace.response(None, OneOrMany::one(AssistantContent::ToolCall(secret)));
        let json = to_string(&trace.drain()).unwrap();
        assert!(!json.contains("sensitive"));
        assert!(json.contains("redacted"));
    }
}
