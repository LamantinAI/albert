use octo_connector_telegram::{IngressScope, TelegramOutputScope};
use octo_core::{ChannelId, ConnectorId, Envelope, EventKind};

use crate::{
    messaging::{
        contract::MessageIntent,
        store::MessageRecord,
        transport::{AdapterKind, ReportAttempt},
    },
    sessions::{presentation::OrdinarySessionOutput, SessionContext},
};

/// Fresh host output; canonical body and its work identity remain unchanged.
pub(super) fn report_envelope(
    source: &ConnectorId,
    context: SessionContext,
    record: &MessageRecord,
    receipt: &ReportAttempt,
) -> Envelope {
    let execution = record
        .envelope
        .execution
        .connector
        .as_deref()
        .map(|connector| format!("host / {connector}"))
        .unwrap_or_else(|| "host".into());
    let implicit = record.envelope.intent == MessageIntent::Response;
    let label = if implicit { "Reply" } else { "Message" };
    let text = format!(
        "{label} from session {} · {execution}\n\n{}\n\nSource session: {}",
        record.envelope.source, record.content, record.envelope.source
    );
    let mut envelope = Envelope::new(source.clone(), EventKind::new("chat.reply"), text)
        .with_target(ConnectorId::new(&receipt.connector))
        .with_channel(ChannelId::new(&receipt.channel))
        .with_context(context)
        .with_context(receipt.clone())
        .with_context(TelegramOutputScope {
            scope: IngressScope {
                key: format!("albert/report/{}", receipt.message.as_str()),
                epoch: receipt.token.clone(),
                ephemeral: false,
                cleanup_scope: None,
            },
            attempt_id: Some(receipt.token.clone()),
        });
    if implicit && receipt.adapter == AdapterKind::Telegram {
        envelope.context.insert(OrdinarySessionOutput);
    }
    envelope
}
