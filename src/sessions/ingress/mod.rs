//! Trusted transport admission. Text alone does not create a session: the
//! connector must advertise chat messages and classify the physical channel.

use octo_core::{CogitatorContext, Envelope};

use crate::{
    acl::tag,
    sessions::{
        error::{SessionError, SessionResult},
        Admission, AdmissionScope, ExternalIdentity, ReplyRoute,
    },
};

pub(crate) fn for_message(incoming: &Envelope, ctx: &CogitatorContext) -> SessionResult<Admission> {
    let supports_dialog = ctx.connectors().iter().any(|connector| {
        connector.id == incoming.source
            && connector
                .capabilities
                .event_kinds_emit
                .iter()
                .any(|kind| kind.as_str() == "chat.message")
    });
    if !supports_dialog {
        return Err(SessionError::InvalidInput(
            "this connector does not support dialog sessions",
        ));
    }
    from_verified_adapter(incoming)
}

fn from_verified_adapter(incoming: &Envelope) -> SessionResult<Admission> {
    if incoming.kind.as_str() != "chat.message" {
        return Err(SessionError::InvalidInput("not a dialog message"));
    }
    let channel = incoming
        .channel
        .as_ref()
        .ok_or(SessionError::InvalidInput("dialog channel is missing"))?;
    let route = ReplyRoute::new(
        incoming.source.as_str(),
        channel.as_str(),
        Some(incoming.id.to_string()),
    )?;
    let identity = tag(incoming, "sender_id")
        .map(|actor| ExternalIdentity::new(incoming.source.as_str(), actor))
        .transpose()?;
    match tag(incoming, "chat_type") {
        Some("private") => Admission::new(
            identity.ok_or(SessionError::InvalidInput("private actor is missing"))?,
            route,
            AdmissionScope::Private,
            "host",
        ),
        Some("group" | "supergroup") => Admission::group(identity, route, "host"),
        _ => Err(SessionError::InvalidInput(
            "dialog channel classification is missing",
        )),
    }
}

#[cfg(test)]
mod tests {
    use octo_core::{ChannelId, ChannelMetadata, ConnectorId, Envelope, EventKind};

    use super::from_verified_adapter;
    use crate::sessions::AdmissionScope;

    fn message(channel: &str, kind: &str, actor: Option<&str>) -> Envelope {
        let mut metadata = ChannelMetadata::default().with_tag("chat_type", kind);
        if let Some(actor) = actor {
            metadata = metadata.with_tag("sender_id", actor);
        }
        Envelope::new(
            ConnectorId::new("telegram"),
            EventKind::new("chat.message"),
            "hello".to_string(),
        )
        .with_channel(ChannelId::new(channel))
        .with_channel_metadata(metadata)
    }

    #[test]
    fn channel_type_comes_from_adapter_not_numeric_id() {
        let private = from_verified_adapter(&message("-42", "private", Some("7"))).unwrap();
        assert_eq!(private.scope, AdmissionScope::Private);
        assert_eq!(private.route.channel(), "-42");
        let group = from_verified_adapter(&message("42", "supergroup", Some("7"))).unwrap();
        assert_eq!(group.scope, AdmissionScope::Group);
        assert_eq!(group.route.connector(), "telegram");
    }

    #[test]
    fn anonymous_group_does_not_invent_a_private_actor() {
        let group = from_verified_adapter(&message("-42", "group", None)).unwrap();
        assert_eq!(group.scope, AdmissionScope::Group);
        assert!(group.identity.is_none());
        assert!(from_verified_adapter(&message("7", "private", None)).is_err());
    }

    #[test]
    fn arbitrary_text_and_unclassified_channels_are_not_dialog_sessions() {
        let mut event = message("calendar", "private", Some("7"));
        event.kind = EventKind::new("caldav.event");
        assert!(from_verified_adapter(&event).is_err());
        assert!(from_verified_adapter(&message("7", "mail", Some("7"))).is_err());
    }
}
