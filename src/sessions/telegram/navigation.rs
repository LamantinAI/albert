//! One host event path for slash commands and verified picker clicks.
use octo_connector_telegram::ClassicMarkdown;
use octo_core::{ChannelId, ConnectorId, Envelope, EventKind};
use serde_json::json;

use super::output_scope;
use crate::{
    service::session::SessionService,
    sessions::{
        presentation::{NavigationNotice, PresentationOpened},
        SessionContext,
    },
};

pub(super) fn transition_events(
    service: &SessionService,
    previous: &SessionContext,
    current: &SessionContext,
    opened: bool,
) -> Vec<Envelope> {
    let mut events = Vec::new();
    if let Some(detached) = service.presentations().event_for(previous, current) {
        events.push(
            Envelope::new(
                ConnectorId::new("albert.sessions"),
                EventKind::new("albert.presentation.detached"),
                json!({}),
            )
            .with_context(detached),
        );
    }
    if opened {
        events.push(
            Envelope::new(
                ConnectorId::new("albert.sessions"),
                EventKind::new("albert.presentation.opened"),
                json!({}),
            )
            .with_context(PresentationOpened(current.clone())),
        );
    }
    events
}
pub(super) fn navigation_notice(context: &SessionContext, text: String) -> Envelope {
    Envelope::new(
        ConnectorId::new("albert.sessions"),
        EventKind::new("chat.reply"),
        text,
    )
    .with_target(ConnectorId::new(context.reply_route().connector()))
    .with_channel(ChannelId::new(context.reply_route().channel()))
    .with_tag("control_reply", "true")
    .with_context(context.clone())
    .with_context(output_scope(context, None))
    .with_context(NavigationNotice)
    .with_context(ClassicMarkdown)
}
