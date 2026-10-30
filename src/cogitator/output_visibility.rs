use octo_core::Envelope;

use super::AlbertCogitator;
use crate::{messaging::transport::AdapterKind, sessions::SessionContext};

impl AlbertCogitator {
    /// Only the known ordinary-native output path calls this. SessionContext by
    /// itself is not an output intent: notifications/approvals remain separate.
    pub(super) async fn ordinary_output_visible(&self, incoming: &Envelope) -> bool {
        let (Some(service), Some(context)) = (
            &self.session_service,
            incoming.context.get::<SessionContext>(),
        ) else {
            return true;
        };
        if !context.is_private()
            || service
                .delivery_adapters()
                .get(context.reply_route().connector())
                != Some(AdapterKind::Telegram)
        {
            return true;
        }
        service.refresh_presentation(context).await.unwrap_or(false)
            && service.presentations().active(context)
    }
}
