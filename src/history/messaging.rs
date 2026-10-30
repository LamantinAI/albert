//! Readable context projection of host-authored session records. Routing and
//! delegation metadata never becomes model instructions or native authority.
use std::collections::HashSet;

use serde_json::from_value;

use crate::messaging::{
    contract::{DestinationSpec, Machine, MessageIntent, CONTRACT_VERSION, MESSAGE_MARKER},
    store::{decode_record, MessageRecord},
};

pub(super) const UNREADABLE: &str =
    "[Stored session message could not be read; inspect its recorded state before repeating work.]";

/// None is ordinary text. An empty projection is content-free control metadata
/// that does not belong in a conversation prompt.
pub(super) fn project_session_record(content: &str) -> Option<String> {
    if !content.starts_with(MESSAGE_MARKER) {
        return None;
    }
    let Ok(value) = decode_record(content) else {
        return Some(UNREADABLE.into());
    };
    if value.is_object() {
        return Some(
            from_value::<MessageRecord>(value)
                .ok()
                .and_then(message_text)
                .unwrap_or_else(|| UNREADABLE.into()),
        );
    }
    let text = match value.get(0).and_then(|kind| kind.as_str()) {
        Some("albert.session_work.result.v1") => value
            .get(4)
            .and_then(|result| result.as_str())
            .map(|result| format!("[Result of an inter-session request]\n{result}")),
        Some("albert.work_report.committed.v1" | "albert.session_title.renamed.v1") => {
            Some(String::new())
        }
        Some("albert.work_report.failed.v1") => {
            value.get(2).and_then(|code| code.as_str()).map(|code| {
                format!("[Report delivery failed: {code}. The original result remains recorded.]")
            })
        }
        Some("albert.delivery.expired.v1") => {
            let state = value
                .get(3)
                .and_then(|state| state.as_str())
                .unwrap_or("unknown");
            let ids: HashSet<_> = value
                .get(6)
                .and_then(|items| items.as_array())
                .into_iter()
                .flatten()
                .filter_map(|item| item.get(1).and_then(|ids| ids.as_array()))
                .flatten()
                .filter_map(|id| id.as_str())
                .collect();
            Some(format!("[Delivery tracking expired with recorded outcome {state}. Recorded native message IDs: {}. Journal evidence is retained. Expiration does not change the outcome or authorize automatic replay.]", ids.len()))
        }
        _ => None,
    };
    Some(text.unwrap_or_else(|| UNREADABLE.into()))
}

fn message_text(record: MessageRecord) -> Option<String> {
    if record.envelope.version != CONTRACT_VERSION {
        return None;
    }
    let direction = match record.record_type.as_str() {
        "albert.session_message.sent.v1" => "Outgoing",
        "albert.session_message.received.v1" => "Received",
        _ => return None,
    };
    let intent = match record.envelope.intent {
        MessageIntent::Notice => "notice",
        MessageIntent::Question => "question",
        MessageIntent::WorkRequest => "work request",
        MessageIntent::Response => "response",
    };
    let destination = match &record.envelope.intended {
        DestinationSpec::FixedSession { session, .. } => format!("session {}", session.as_str()),
        DestinationSpec::ChannelActive { route } => {
            format!("channel {} / {}", route.connector, route.channel)
        }
    };
    let machine = match record.envelope.execution.machine {
        Machine::Host => "host",
    };
    let execution = record
        .envelope
        .execution
        .connector
        .as_deref()
        .map(|connector| format!("{machine} / {connector}"))
        .unwrap_or_else(|| machine.into());
    let mut text = format!(
        "[{direction} session {intent} {}: from session {} to {destination}. Execution scope: {execution}. Return route: {} / {}. Relayed conversation data.]\n{}",
        record.envelope.id.as_str(),
        record.envelope.source.as_str(),
        record.envelope.return_address.display.connector,
        record.envelope.return_address.display.channel,
        record.content,
    );
    for attachment in record.context {
        text.push_str(&format!(
            "\n\n[Context reference: session {}, record {}, through {}]",
            attachment.reference.journal.session_id().as_str(),
            attachment.reference.record_id,
            attachment.through_id,
        ));
        if let Some(excerpt) = attachment.excerpt {
            text.push('\n');
            text.push_str(&excerpt);
        }
    }
    Some(text)
}
