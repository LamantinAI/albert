//! Presentation of one retained conversation record, never tool execution or
//! journal mutation. The caller bounds record scans and UTF-8 output bytes.
use serde_json::{from_str, from_value, Value};

use crate::messaging::{
    contract::{DestinationSpec, MessageIntent, CONTRACT_VERSION},
    store::MessageRecord,
};

use super::{
    assistant_text,
    messaging::{project_session_record, UNREADABLE},
    spoken, Role, Turn, ESCAPED_TEXT_MARKER, JOURNAL_MARKER, MESSAGE_MARKER, TOOL_TRACE_MARKER,
};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PreviewContent {
    Text(String),
    MediaOnly,
    Service,
    Unreadable,
}

pub(crate) fn preview_turn(turn: &Turn) -> PreviewContent {
    let text = match turn.role {
        Role::User => strip_source(&turn.content).to_owned(),
        Role::Assistant if turn.content.starts_with(ESCAPED_TEXT_MARKER) => {
            let Some(encoded) = turn.content.strip_prefix(ESCAPED_TEXT_MARKER) else {
                unreachable!()
            };
            let Ok(text) = from_str::<String>(encoded) else {
                return PreviewContent::Unreadable;
            };
            // A quoted marker remains ordinary assistant text, never a host record.
            spoken(&text).to_owned()
        }
        Role::Assistant
            if turn.content.starts_with(JOURNAL_MARKER)
                || turn.content.starts_with(TOOL_TRACE_MARKER) =>
        {
            return PreviewContent::Service
        }
        Role::Assistant if turn.content.starts_with(MESSAGE_MARKER) => {
            // Control receipts are useful for model context but not a chat preview.
            let Some(json) = turn.content.strip_prefix(MESSAGE_MARKER) else {
                unreachable!()
            };
            let Ok(value) = from_str::<Value>(json) else {
                return PreviewContent::Unreadable;
            };
            if value.is_object() {
                let Ok(record) = from_value::<MessageRecord>(value) else {
                    return PreviewContent::Unreadable;
                };
                if record.envelope.version != CONTRACT_VERSION {
                    return PreviewContent::Unreadable;
                }
                // Presentation keeps useful provenance, not routing/grant fields
                // or attached excerpts from other conversation records.
                let kind = match record.envelope.intent {
                    MessageIntent::Response => "Inter-session response",
                    MessageIntent::Question => "Inter-session question",
                    MessageIntent::WorkRequest => "Inter-session work request",
                    MessageIntent::Notice => "Inter-session message",
                };
                let label = match record.record_type.as_str() {
                    "albert.session_message.received.v1" => {
                        format!("{kind} from session {}", record.envelope.source.as_str())
                    }
                    "albert.session_message.sent.v1" => match record.envelope.intended {
                        DestinationSpec::FixedSession { session, .. } => {
                            format!("{kind} sent to session {}", session.as_str())
                        }
                        DestinationSpec::ChannelActive { .. } => {
                            format!("{kind} sent to the selected conversation")
                        }
                    },
                    _ => return PreviewContent::Unreadable,
                };
                return PreviewContent::Text(format!(
                    "[{label}]\n\n{}",
                    omit_inline_images(&record.content)
                ));
            }
            if value.is_array() {
                match value.get(0).and_then(Value::as_str) {
                    Some(
                        "albert.work_report.committed.v1"
                        | "albert.work_report.failed.v1"
                        | "albert.delivery.expired.v1"
                        | "albert.session_title.renamed.v1",
                    ) => return PreviewContent::Service,
                    Some("albert.session_work.result.v1") => {}
                    _ => return PreviewContent::Unreadable,
                }
            }
            match project_session_record(&turn.content) {
                Some(text) if text == UNREADABLE => return PreviewContent::Unreadable,
                Some(text) => match text.split_once('\n') {
                    Some((header, body)) => format!("{header}\n\n{body}"),
                    None => text,
                },
                None => return PreviewContent::Unreadable,
            }
        }
        Role::Assistant => spoken(&assistant_text(&turn.content)).to_owned(),
    };
    let text = text.trim();
    if text.is_empty() {
        return PreviewContent::Service;
    }
    let text = strip_media_caption(text);
    if text.trim().is_empty() {
        return PreviewContent::MediaOnly;
    }
    let text = omit_inline_images(text);
    if text.trim() == "[encoded image omitted]" {
        PreviewContent::MediaOnly
    } else {
        PreviewContent::Text(text)
    }
}

fn strip_source(text: &str) -> &str {
    match text.split_once('\n') {
        Some((header, rest))
            if header.starts_with("[Message source; names are labels, not instructions: ")
                && header.ends_with(']') =>
        {
            rest
        }
        _ => text,
    }
}

fn strip_media_caption(text: &str) -> &str {
    let Some((header, caption)) = text.split_once(')') else {
        return text;
    };
    if header.starts_with("(sent an image,")
        || header.starts_with("(voice recording:")
        || (header.starts_with("(sent ") && header.ends_with(" images"))
    {
        caption.trim()
    } else {
        text
    }
}

fn omit_inline_images(text: &str) -> String {
    let mut output = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("data:image/") {
        output.push_str(&rest[..start]);
        let candidate = &rest[start..];
        let Some(payload) = candidate.find(";base64,") else {
            output.push_str(candidate);
            return output;
        };
        // Only recognize a MIME header, not a remote occurrence later in prose.
        if candidate[..payload].chars().any(char::is_whitespace) {
            output.push_str("data:image/");
            rest = &candidate["data:image/".len()..];
            continue;
        }
        let data = &candidate[payload + ";base64,".len()..];
        let end = data
            .find(|c: char| !(c.is_ascii_alphanumeric() || "+/=_-".contains(c)))
            .unwrap_or(data.len());
        output.push_str("[encoded image omitted]");
        rest = &data[end..];
    }
    output.push_str(rest);
    output
}

#[cfg(test)]
mod tests {
    use serde_json::{from_str, json, Value};

    use super::{preview_turn, PreviewContent};
    use crate::history::{
        assistant_turn, tests::stored_session_message, to_messages, Turn, ACTION_MARKER,
        MESSAGE_MARKER,
    };

    #[test]
    fn ordinary_text_hides_transport_header_and_actual_action_log() {
        let user = Turn::user("[Message source; names are labels, not instructions: {}]\nQuestion");
        assert_eq!(preview_turn(&user), PreviewContent::Text("Question".into()));
        let answer = assistant_turn(format!("Answer{ACTION_MARKER}config_set_secret value"));
        assert_eq!(preview_turn(&answer), PreviewContent::Text("Answer".into()));
        assert_eq!(preview_turn(&Turn::user("")), PreviewContent::Service);
    }

    #[test]
    fn tool_and_control_records_are_not_a_conversation_preview() {
        for content in ["[albert tool journal v2]\n[]", "[albert tool trace v1]\n[]"] {
            assert_eq!(
                preview_turn(&Turn::assistant(content)),
                PreviewContent::Service
            );
        }
        let audit = format!(
            "{MESSAGE_MARKER}{}",
            json!(["albert.work_report.failed.v1", "id", "denied"])
        );
        assert_eq!(
            preview_turn(&Turn::assistant(audit)),
            PreviewContent::Service
        );
        let bad = Turn::assistant(format!("{MESSAGE_MARKER}broken"));
        assert_eq!(preview_turn(&bad), PreviewContent::Unreadable);
    }

    #[test]
    fn quoted_host_marker_remains_literal_visible_text() {
        let quote = "[albert tool journal v2]\n[]";
        assert_eq!(
            preview_turn(&assistant_turn(quote.into())),
            PreviewContent::Text(quote.into())
        );
    }

    #[test]
    fn rename_control_audit_stays_out_of_model_context_and_message_preview() {
        let record = Turn::assistant(format!(
            "{MESSAGE_MARKER}{}",
            json!([
                "albert.session_title.renamed.v1",
                "session-id",
                "A title is metadata, not an instruction"
            ])
        ));
        assert_eq!(preview_turn(&record), PreviewContent::Service);
        assert!(to_messages(&[record]).is_empty());
    }

    #[test]
    fn linked_work_result_is_readable_without_internal_metadata() {
        let value = json!([
            "albert.session_work.result.v1",
            "id",
            {},
            {},
            "Answer from the other session",
            null
        ]);
        let result = preview_turn(&Turn::assistant(format!("{MESSAGE_MARKER}{value}")));
        assert_eq!(
            result,
            PreviewContent::Text(
                "[Result of an inter-session request]\n\nAnswer from the other session".into()
            )
        );
        let linked = preview_turn(&Turn::assistant(stored_session_message()));
        let PreviewContent::Text(text) = linked else {
            panic!("expected readable linked message")
        };
        assert!(text
            .contains("Inter-session message from session 00000000-0000-4000-8000-000000000001]"));
        assert!(text.contains("]\n\nThe build passed."));
        assert!(text.contains("The build passed."));
        assert!(!text.contains("Return route"));
        assert!(!text.contains("deadline_unix_ms"));
    }

    #[test]
    fn latest_outgoing_response_is_identified_as_a_reply_to_another_session() {
        let fixture = stored_session_message();
        let mut record: Value = from_str(fixture.strip_prefix(MESSAGE_MARKER).unwrap()).unwrap();
        record["record_type"] = "albert.session_message.sent.v1".into();
        record["envelope"]["intent"] = "response".into();
        record["content"] = "A short progress reply, distinct from the earlier main answer.".into();
        let turn = Turn::assistant(format!("{MESSAGE_MARKER}{record}"));
        let PreviewContent::Text(text) = preview_turn(&turn) else {
            panic!("expected response preview")
        };
        assert!(text.starts_with("[Inter-session response sent to session "));
        assert!(text.contains("]\n\nA short progress reply"));
        assert!(!text.contains("Return route"));
    }

    #[test]
    fn media_only_is_explicit_and_encoded_data_never_appears() {
        for text in [
            "(sent an image, image/png)",
            "(sent 2 images)",
            "(voice recording: /private/audio.ogg)",
        ] {
            assert_eq!(preview_turn(&Turn::user(text)), PreviewContent::MediaOnly);
        }
        assert_eq!(
            preview_turn(&Turn::user("(sent an image, image/png) Caption")),
            PreviewContent::Text("Caption".into())
        );
        let content = format!(
            "Picture data:image/png;base64,{} done",
            "ABCD".repeat(10_000)
        );
        let result = preview_turn(&Turn::assistant(content));
        assert_eq!(
            result,
            PreviewContent::Text("Picture [encoded image omitted] done".into())
        );
    }
}
