use std::io::{Error, ErrorKind, Result};

use octo_core::{Blob, Envelope};
use tokio::io::{AsyncWrite, AsyncWriteExt};

use crate::messaging::transport::{AdapterKind, ReportAttempt};

pub(super) async fn write_reply<W: AsyncWrite + Unpin>(
    writer: &mut W,
    envelope: &Envelope,
    connector: &str,
) -> Result<()> {
    let attempt = envelope.context.get::<ReportAttempt>();
    if attempt.is_some_and(|attempt| {
        attempt.adapter != AdapterKind::Console
            || attempt.connector != connector
            || attempt.channel != "stdin"
            || envelope
                .target
                .as_ref()
                .is_none_or(|target| target.as_str() != connector)
            || envelope
                .channel
                .as_ref()
                .is_none_or(|channel| channel.as_str() != "stdin")
    }) {
        return Err(Error::new(
            ErrorKind::PermissionDenied,
            "invalid console report attempt",
        ));
    }
    let text = if let Some(blob) = envelope.payload_as::<Blob>() {
        format!("[media: {}, {} bytes]\n", blob.content_type(), blob.len())
    } else if let Some(text) = envelope.payload_as::<String>() {
        format!("bot: {text}\n")
    } else {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "unsupported console output",
        ));
    };
    let outcome = async {
        writer.write_all(text.as_bytes()).await?;
        writer.flush().await
    }
    .await;
    if let Some(attempt) = attempt {
        attempt.console(outcome.is_ok()).await;
    }
    outcome
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use octo_core::{ChannelId, ConnectorId, Envelope, EventKind};
    use octo_history::SqliteHistory;
    use sqlx::sqlite::SqlitePoolOptions;
    use tokio::io::{duplex, AsyncReadExt};

    use super::write_reply;
    use crate::{
        messaging::{
            contract::{
                DestinationSpec, DisplayRoute, ExecutionLocation, MessageDraft, MessageId,
                MessageIntent, TrustedSource, WorkRef,
            },
            policy::{MessagingLimits, RouteGuard},
            store::{DeliveryState, MessageStore},
            transport::{AdapterKind, ReportAttempt},
            MessagingResult,
        },
        service::session::SessionService,
        sessions::{
            settings::SessionSettings, Admission, AdmissionScope, ExternalIdentity, ReplyRoute,
        },
    };
    struct Guard;
    impl RouteGuard for Guard {
        fn check(&self, _: &str, _: &DisplayRoute) -> MessagingResult<()> {
            Ok(())
        }
    }
    async fn fixture() -> (MessageStore, TrustedSource, MessageId, Envelope) {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let service = SessionService::from_history(
            Arc::new(SqliteHistory::from_pool(pool).await.unwrap()),
            SessionSettings::default(),
        )
        .await
        .unwrap();
        let context = service
            .admit(
                Admission::new(
                    ExternalIdentity::new("console", "local").unwrap(),
                    ReplyRoute::new("console", "stdin", None).unwrap(),
                    AdmissionScope::Private,
                    "host",
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let recipient = service.create_private(&context, "Recipient").await.unwrap();
        let source = TrustedSource::current(
            &context,
            WorkRef::Turn("existing-turn".into()),
            ExecutionLocation::default(),
        )
        .unwrap();
        let store = MessageStore::new(
            service.history().clone(),
            service.journal_router().lifecycle_gate(),
            MessagingLimits::default(),
        )
        .unwrap();
        let proposal = MessageDraft {
            id: MessageId::new(),
            destination: DestinationSpec::FixedSession {
                session: recipient.session_id().clone(),
                display: Some(DisplayRoute {
                    connector: "console".into(),
                    channel: "stdin".into(),
                }),
            },
            intent: MessageIntent::Notice,
            content: "report".into(),
            correlation: None,
            causation: None,
            deadline_unix_ms: 1000,
            hops_remaining: 1,
            context: vec![],
            ceiling: None,
        };
        store.send(&source, &proposal, &Guard, 1).await.unwrap();
        store.dispatch(&proposal.id, &Guard, 2).await.unwrap();
        let started = store
            .begin_delivery(&proposal.id, &Guard, 3, "console", 100)
            .await
            .unwrap();
        let attempt = ReportAttempt::new(
            store.clone(),
            proposal.id.clone(),
            started.token,
            AdapterKind::Console,
            "console".into(),
            "stdin".into(),
            8,
        );
        let envelope = Envelope::new(
            ConnectorId::new("albert"),
            EventKind::new("chat.reply"),
            "report".to_owned(),
        )
        .with_target(ConnectorId::new("console"))
        .with_channel(ChannelId::new("stdin"))
        .with_context(attempt);
        (store, source, proposal.id, envelope)
    }
    #[tokio::test]
    async fn successful_write_and_flush_confirm_stream_output_only() {
        let (store, source, id, envelope) = fixture().await;
        let (mut writer, mut reader) = duplex(1024);
        write_reply(&mut writer, &envelope, "console")
            .await
            .unwrap();
        drop(writer);
        let mut text = String::new();
        reader.read_to_string(&mut text).await.unwrap();
        assert_eq!(text, "bot: report\n");
        let status = store.status(&source, &id).await.unwrap();
        assert_eq!(status.delivery, DeliveryState::Delivered);
        assert!(status
            .physical
            .completion_note
            .contains("human reading is not confirmed"));
    }
    #[tokio::test]
    async fn write_failure_is_uncertain_and_does_not_claim_delivery_or_auto_retry() {
        let (store, source, id, envelope) = fixture().await;
        let (mut writer, reader) = duplex(1);
        drop(reader);
        assert!(write_reply(&mut writer, &envelope, "console")
            .await
            .is_err());
        let status = store.status(&source, &id).await.unwrap();
        assert_eq!(status.delivery, DeliveryState::Unknown);
        assert_eq!(status.physical.observations[0].outcome, "stream_unknown");
        assert!(store.delivery_page(None, 16).await.unwrap().is_empty());
    }
}
