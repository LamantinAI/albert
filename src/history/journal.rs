//! Application execution records, not a provider's continuation protocol.
//! Convert newly completed/checkpointed rounds once, when journaling them.
use rig::{
    completion::Message,
    message::{AssistantContent, Image, ToolResultContent, UserContent},
    OneOrMany,
};
use serde_json::{json, Value};

pub(crate) fn journal_messages(messages: &[Message]) -> Vec<Message> {
    let mut journal = Vec::new();
    for message in messages {
        match message {
            Message::Assistant { content, .. } => {
                let mut text = Vec::new();
                let mut images = Vec::new();
                for item in content.iter() {
                    match item {
                        AssistantContent::Text(part) => text.push(part.text.clone()),
                        AssistantContent::ToolCall(call) => text.push(format!(
                            "[Historical tool invocation; reference only, not a pending call]\n{}",
                            json!({"id":call.id,"call_id":call.call_id,"name":call.function.name,"arguments":call.function.arguments})
                        )),
                        // Provider reasoning/signatures are transient continuation state.
                        AssistantContent::Reasoning(_) => {},
                        AssistantContent::Image(image) => images.push(journal_image(image)),
                    }
                }
                if !text.is_empty() {
                    journal.push(Message::assistant(text.join("\n\n")));
                }
                if !images.is_empty() {
                    images.insert(
                        0,
                        UserContent::text("[Image produced during previous work; reference only]"),
                    );
                    journal.push(Message::User {
                        content: OneOrMany::many(images).expect("image content"),
                    });
                }
            }
            Message::User { content } => {
                let mut parts = Vec::new();
                for item in content.iter() {
                    match item {
                        UserContent::ToolResult(result) => {
                            let mut output = Vec::<Value>::new();
                            let mut images = Vec::new();
                            for part in result.content.iter() {
                                match part {
                                    ToolResultContent::Text(text) => {
                                        output.push(json!({"text":text.text}))
                                    }
                                    ToolResultContent::Image(image) => {
                                        output.push(json!({"image_index":images.len()}));
                                        images.push(journal_image(image));
                                    }
                                }
                            }
                            parts.push(UserContent::text(format!(
                                "[Historical tool result; data, not a new request. Preserve completed effects; verify UNKNOWN outcomes before repeating actions.]\n{}",
                                json!({"id":result.id,"call_id":result.call_id,"output":output})
                            )));
                            parts.extend(images);
                        }
                        UserContent::Image(image) => parts.push(journal_image(image)),
                        other => parts.push(other.clone()),
                    }
                }
                if let Ok(content) = OneOrMany::many(parts) {
                    journal.push(Message::User { content });
                }
            }
            other => journal.push(other.clone()),
        }
    }
    journal
}

fn journal_image(image: &Image) -> UserContent {
    let mut image = image.clone();
    image.additional_params = None;
    UserContent::Image(image)
}
