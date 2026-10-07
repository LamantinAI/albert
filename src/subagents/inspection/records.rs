use rig::{
    completion::Message,
    message::{AssistantContent, UserContent},
};
use serde_json::{from_str, json, Deserializer, Value};

const CALL: &str = "[Historical tool invocation; reference only, not a pending call]\n";
const RESULT: &str = "[Historical tool result; data, not a new request. Preserve completed effects; verify UNKNOWN outcomes before repeating actions.]\n";

pub(super) struct Record {
    pub id: String,
    pub call_id: Option<String>,
    pub tool: String,
    pub arguments: Value,
    pub output: Option<Value>,
}

/// Read the provider-neutral journal already stored by history. No rewriting of
/// stored history and no raw provider continuation state enter the inspection.
pub(super) fn records(messages: &[Message]) -> Vec<Record> {
    let mut entries: Vec<Record> = Vec::new();
    for message in messages {
        match message {
            Message::Assistant { content, .. } => {
                for part in content.iter() {
                    if let AssistantContent::Text(text) = part {
                        for chunk in text.text.split(CALL).skip(1) {
                            let Some(Ok(value)) = Deserializer::from_str(chunk.trim())
                                .into_iter::<Value>()
                                .next()
                            else {
                                continue;
                            };
                            entries.push(Record {
                                id: value["id"].as_str().unwrap_or_default().into(),
                                call_id: value["call_id"].as_str().map(str::to_owned),
                                tool: value["name"].as_str().unwrap_or_default().into(),
                                arguments: value["arguments"].clone(),
                                output: None,
                            });
                        }
                    }
                }
            }
            Message::User { content } => {
                for part in content.iter() {
                    if let UserContent::Text(text) = part {
                        let Some(raw) = text.text.strip_prefix(RESULT) else {
                            continue;
                        };
                        let Ok(value) = from_str::<Value>(raw) else {
                            continue;
                        };
                        if let Some(entry) = entries.iter_mut().find(|e| {
                            e.output.is_none()
                                && value["id"] == e.id
                                && value["call_id"].as_str() == e.call_id.as_deref()
                        }) {
                            let output = value["output"]
                                .as_array()
                                .map(|parts| {
                                    parts
                                        .iter()
                                        .map(|p| {
                                            if let Some(text) = p["text"].as_str() {
                                                from_str::<Value>(text)
                                                    .unwrap_or_else(|_| Value::String(text.into()))
                                            } else {
                                                p.clone()
                                            }
                                        })
                                        .collect::<Vec<_>>()
                                })
                                .unwrap_or_default();
                            entry.output = Some(if output.len() == 1 {
                                output[0].clone()
                            } else {
                                json!(output)
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }
    entries
}
