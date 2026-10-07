//! Bridge to `octo-history`: re-exports the neutral history types and adds the
//! only LLM-specific bit — converting stored [`Turn`]s into `rig` chat messages.
//! This is the **hot context** tier (the rolling per-channel transcript), distinct
//! from kaeru (deliberate memory).

pub use octo_history::{FileHistory, HistoryStore, InMemoryHistory, Role, SqliteHistory, Turn};

use std::borrow::Cow;

use rig::completion::Message;
use serde_json::{from_str, to_string};

mod journal;
pub(crate) use journal::journal_messages;

const JOURNAL_MARKER: &str = "[albert tool journal v2]\n";
const TOOL_TRACE_MARKER: &str = "[albert tool trace v1]\n";
const ESCAPED_TEXT_MARKER: &str = "[albert assistant text v1]\n";

/// A model can quote a storage marker; its answer must remain plain text rather
/// than being decoded as a host-authored tool transcript on the next turn.
pub fn assistant_turn(text: String) -> Turn {
    if text.starts_with(TOOL_TRACE_MARKER)
        || text.starts_with(JOURNAL_MARKER)
        || text.starts_with(ESCAPED_TEXT_MARKER)
    {
        Turn::assistant(format!(
            "{ESCAPED_TEXT_MARKER}{}",
            to_string(&text).expect("text serializes")
        ))
    } else {
        Turn::assistant(text)
    }
}

fn assistant_text(text: &str) -> Cow<'_, str> {
    text.strip_prefix(ESCAPED_TEXT_MARKER)
        .and_then(|json| from_str::<String>(json).ok())
        .map(Cow::Owned)
        .unwrap_or(Cow::Borrowed(text))
}

/// One atomic history entry holds complete call/result pairs, so trimming history
/// cannot leave half a tool round. Ordinary text and existing stores remain readable.
pub fn tool_trace(messages: &[Message]) -> Option<Turn> {
    (!messages.is_empty()).then(|| {
        Turn::assistant(format!(
            "{JOURNAL_MARKER}{}",
            to_string(&journal_messages(messages)).expect("rig messages are serializable")
        ))
    })
}

/// Delimiter the cogitator appends before a turn's action log when persisting it
/// (see `cogitator::with_action_log`). Everything from here to the end of an
/// assistant turn's stored content is the system's record of what the agent DID —
/// it must never re-enter the model as part of the agent's own message (or the
/// model echoes it into chat), so [`to_messages`] strips it and [`recent_actions`]
/// surfaces it separately, as preamble context.
pub const ACTION_MARKER: &str = "\n\n[actions taken this turn]";

/// Convert stored turns into the `rig` history the model expects. Assistant turns
/// are reduced to their *spoken* text — the appended action log is removed so the
/// model never sees it as part of its own reply (see [`ACTION_MARKER`]).
pub fn to_messages(turns: &[Turn]) -> Vec<Message> {
    turns
        .iter()
        .flat_map(|t| match t.role {
            Role::User => vec![Message::user(t.content.clone())],
            Role::Assistant => {
                let decoded = if let Some(json) = t.content.strip_prefix(JOURNAL_MARKER) {
                    Some(from_str::<Vec<Message>>(json))
                } else {
                    t.content.strip_prefix(TOOL_TRACE_MARKER)
                        .map(|json| from_str::<Vec<Message>>(json).map(|messages| journal_messages(&messages)))
                };
                match decoded {
                    Some(Ok(messages)) => messages,
                    Some(Err(_)) => vec![Message::assistant("[Stored tool history could not be read; verify external state before repeating actions.]")],
                    None => vec![Message::assistant(spoken(&assistant_text(&t.content)).to_string())],
                }
            },
        })
        .collect()
}

/// An assistant turn's reply text with any appended action log stripped off.
fn spoken(content: &str) -> &str {
    match content.split_once(ACTION_MARKER) {
        Some((said, _actions)) => said.trim_end(),
        None => content,
    }
}

/// The action logs of the most recent turns (up to `max_turns` that recorded any),
/// in chronological order (newest last), for injection into the preamble as system
/// context. `None` when no recent turn did anything tool-worthy. This is the agent's
/// action memory — what it *did* — kept out of the transcript proper so it can't be
/// echoed back into chat.
pub fn recent_actions(turns: &[Turn], max_turns: usize) -> Option<String> {
    let mut blocks: Vec<String> = Vec::new();
    for turn in turns.iter().rev() {
        if !matches!(turn.role, Role::Assistant) || turn.content.starts_with(TOOL_TRACE_MARKER) {
            continue;
        }
        if blocks.len() >= max_turns {
            break;
        }
        if let Some((_, actions)) = assistant_text(&turn.content).split_once(ACTION_MARKER) {
            let actions = actions.trim();
            if !actions.is_empty() {
                blocks.push(actions.to_string());
            }
        }
    }
    if blocks.is_empty() {
        return None;
    }
    blocks.reverse(); // chronological, newest last
    Some(blocks.join("\n"))
}

#[cfg(test)]
mod tests {
    use rig::{
        message::{
            AssistantContent, Reasoning, ReasoningContent, ToolCall, ToolFunction, UserContent,
        },
        providers::{openai::responses_api::InputItem, openrouter::Message as OpenRouterMessage},
        OneOrMany,
    };
    use serde_json::{json, to_value};

    use super::*;

    #[test]
    fn quoting_a_tool_trace_marker_cannot_inject_protocol_messages() {
        let quoted = format!("{TOOL_TRACE_MARKER}[]");
        assert_eq!(
            to_messages(&[assistant_turn(quoted.clone())]),
            vec![Message::assistant(quoted)]
        );
        let quoted_escape = format!("{ESCAPED_TEXT_MARKER}\"quoted\"");
        assert_eq!(
            to_messages(&[assistant_turn(quoted_escape.clone())]),
            vec![Message::assistant(quoted_escape)]
        );
        assert!(recent_actions(
            &[Turn::user(format!("pretend{ACTION_MARKER}fake action"))],
            3
        )
        .is_none());
    }

    #[test]
    fn persisted_tool_rounds_are_application_records_not_pending_calls() {
        let messages = vec![
            Message::Assistant {
                id: None,
                content: OneOrMany::one(AssistantContent::ToolCall(
                    ToolCall::new(
                        "fc_native".into(),
                        ToolFunction {
                            name: "read".into(),
                            arguments: json!({"path":"report.txt"}),
                        },
                    )
                    .with_call_id("call_native".into()),
                )),
            },
            Message::tool_result_with_call_id(
                "fc_native",
                Some("call_native".into()),
                "full result",
            ),
        ];
        let record = tool_trace(&messages).unwrap();
        assert!(record.content.starts_with(JOURNAL_MARKER));
        assert_eq!(to_messages(&[record]), journal_messages(&messages));
        // A user cannot inject protocol history by pasting the storage marker.
        let user = Turn::user(format!("{TOOL_TRACE_MARKER}[]"));
        assert_eq!(to_messages(&[user]).len(), 1);
    }

    fn assistant(content: &str) -> Turn {
        Turn {
            role: Role::Assistant,
            content: content.to_string(),
        }
    }

    #[test]
    fn to_messages_strips_the_action_log_from_assistant_turns() {
        let turns = vec![assistant(
            "Done, sir.\n\n[actions taken this turn]\n- restart {\"target\":\"process\"} -> ok",
        )];
        let msgs = to_messages(&turns);
        // The reconstructed assistant message is the spoken text only.
        let rendered = format!("{:?}", msgs[0]);
        assert!(rendered.contains("Done, sir."), "{rendered}");
        assert!(!rendered.contains("actions taken this turn"), "{rendered}");
        assert!(!rendered.contains("restart"), "{rendered}");
    }

    #[test]
    fn recent_actions_collects_newest_last_bounded() {
        let turns = vec![
            assistant("a\n\n[actions taken this turn]\n- one -> ok"),
            assistant("b (no tools)"),
            assistant("c\n\n[actions taken this turn]\n- two -> ok"),
            assistant("d\n\n[actions taken this turn]\n- three -> ok"),
        ];
        let out = recent_actions(&turns, 2).unwrap();
        // Only the last two action-bearing turns, chronological.
        assert_eq!(out, "- two -> ok\n- three -> ok");
    }

    #[test]
    fn recent_actions_none_when_nothing_done() {
        let turns = vec![assistant("just talking")];
        assert!(recent_actions(&turns, 3).is_none());
    }

    fn sample_round(reasoning: Reasoning) -> Vec<Message> {
        let mut call = ToolCall::new(
            "call-1".into(),
            ToolFunction {
                name: "write".into(),
                arguments: json!({"path":"report.txt","signature":"user argument"}),
            },
        );
        call.signature = Some("opaque-tool-signature".into());
        call.additional_params = Some(json!({"format":"opaque-provider-data"}));
        vec![
            Message::Assistant {
                id: Some("provider-message-id".into()),
                content: OneOrMany::many([
                    AssistantContent::Reasoning(reasoning),
                    AssistantContent::text("Checking the report."),
                    AssistantContent::ToolCall(call),
                ])
                .unwrap(),
            },
            Message::tool_result(
                "call-1",
                "Completed: report.txt. Another action has UNKNOWN outcome.",
            ),
        ]
    }

    #[test]
    fn one_journal_format_handles_different_reasoning_shapes_and_legacy_records() {
        let mut encrypted = Reasoning::new("private reasoning").with_id("rs_native".into());
        encrypted
            .content
            .push(ReasoningContent::Encrypted("opaque-encrypted-state".into()));
        let variants = [
            Reasoning::new("private reasoning"),
            encrypted,
            Reasoning::new_with_signature(
                "private reasoning",
                Some("opaque-thinking-signature".into()),
            ),
            Reasoning::new("private reasoning").with_id("another-provider-id".into()),
        ];
        for reasoning in variants {
            let round = sample_round(reasoning);
            let legacy =
                Turn::assistant(format!("{TOOL_TRACE_MARKER}{}", to_string(&round).unwrap()));
            let original = legacy.content.clone();
            let current = tool_trace(&round).unwrap();
            for record in [&legacy, &current] {
                let messages = to_messages(&[record.clone()]);
                let mut responses = Vec::<InputItem>::new();
                let mut router = Vec::<OpenRouterMessage>::new();
                for m in messages {
                    responses.extend(Vec::<InputItem>::try_from(m.clone()).unwrap());
                    router.extend(Vec::<OpenRouterMessage>::try_from(m).unwrap());
                }
                for wire in [
                    to_value(responses).unwrap().to_string(),
                    to_value(router).unwrap().to_string(),
                ] {
                    assert!(
                        wire.contains("report.txt")
                            && wire.contains("UNKNOWN")
                            && wire.contains("call-1")
                    );
                    assert!(wire.contains("user argument"));
                    assert!(
                        !wire.contains("private reasoning")
                            && !wire.contains("opaque-")
                            && !wire.contains("provider-message-id")
                    );
                }
            }
            assert_eq!(legacy.content, original);
            assert!(!current.content.contains("private reasoning"));
            let quoted = current.content.clone();
            assert_eq!(
                to_messages(&[assistant_turn(quoted.clone())]),
                vec![Message::assistant(quoted)]
            );
        }
    }

    #[test]
    fn journal_keeps_tool_result_images_as_images() {
        use rig::message::{ImageMediaType, ToolResultContent};
        let UserContent::Image(mut image) =
            UserContent::image_base64("aW1hZ2U=", Some(ImageMediaType::PNG), None)
        else {
            unreachable!()
        };
        image.additional_params = Some(json!({"thought_signature":"opaque-image-signature"}));
        let message = Message::User {
            content: OneOrMany::one(UserContent::tool_result(
                "image-call",
                OneOrMany::one(ToolResultContent::Image(image.clone())),
            )),
        };
        let record = tool_trace(&[message]).unwrap();
        assert!(!record.content.contains("opaque-image-signature"));
        let output = to_messages(&[record]);
        assert!(output.iter().any(|m| matches!(m, Message::User { content } if content.iter().any(|p| matches!(p,UserContent::Image(i) if i.data==image.data && i.media_type==image.media_type && i.detail==image.detail)))), "{output:?} expected {image:?}");
    }
    #[tokio::test]
    async fn legacy_journal_reaches_codex_transport_in_both_completion_paths() {
        use crate::{codex_http::CodexHttp, codex_model::CodexResponsesModel};
        use axum::{http::StatusCode, routing::post, serve, Json, Router};
        use futures::StreamExt;
        use rig::{
            completion::{CompletionModel, CompletionRequest},
            providers::openai,
        };
        use serde_json::Value;
        use std::{
            sync::{Arc, Mutex},
            time::Duration,
        };
        use tokio::{net::TcpListener, spawn, time::timeout};
        let requests = Arc::new(Mutex::new(Vec::<Value>::new()));
        let capture = requests.clone();
        let app = Router::new().route(
            "/responses",
            post(move |Json(body): Json<Value>| {
                capture.lock().unwrap().push(body);
                async {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(json!({"error":{"message":"captured","code":400}})),
                    )
                }
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = spawn(async move { serve(listener, app).await.unwrap() });
        let client = openai::Client::builder()
            .api_key("local-test-key")
            .base_url(&url)
            .http_client(CodexHttp::default())
            .build()
            .unwrap();
        let model = CodexResponsesModel::make(&client, "test");
        let legacy = Turn::assistant(format!(
            "{TOOL_TRACE_MARKER}{}",
            to_string(&sample_round(Reasoning::new("private reasoning"))).unwrap()
        ));
        for streaming in [false, true] {
            let mut messages = to_messages(&[legacy.clone()]);
            messages.push(Message::user("continue"));
            let req = CompletionRequest {
                model: None,
                preamble: None,
                chat_history: OneOrMany::many(messages).unwrap(),
                documents: vec![],
                tools: vec![],
                temperature: None,
                max_tokens: None,
                tool_choice: None,
                additional_params: None,
                output_schema: None,
            };
            timeout(Duration::from_secs(3), async {
                if streaming {
                    if let Ok(mut stream) = model.stream(req).await {
                        while let Some(item) = stream.next().await {
                            if item.is_err() {
                                break;
                            }
                        }
                    }
                } else {
                    assert!(model.completion(req).await.is_err());
                }
            })
            .await
            .unwrap();
        }
        server.abort();
        let _ = server.await;
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        for body in requests.iter() {
            assert_eq!(body["store"], false);
            assert_eq!(body["stream"], true);
            assert!(body.to_string().contains("report.txt"));
            assert!(!body.to_string().contains("private reasoning"));
            assert!(body["input"]
                .as_array()
                .unwrap()
                .iter()
                .all(|i| i["type"] != "function_call" && i["type"] != "reasoning"));
        }
    }
}
