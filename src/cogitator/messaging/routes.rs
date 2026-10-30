use std::collections::HashSet;

use octo_core::CogitatorContext;
use serde::Serialize;
use sqlx::query_as;

use crate::{
    messaging::{
        contract::DisplayRoute,
        policy::RouteGuard,
        transport::{AdapterKind, AdapterRegistry},
        MessagingError, MessagingResult,
    },
    service::session::SessionService,
};

#[derive(Clone)]
pub(super) struct RuntimeRoutes {
    supported: HashSet<String>,
    adapters: Option<AdapterRegistry>,
}
#[derive(Serialize)]
pub(super) struct RouteView {
    #[serde(skip)]
    pub next_offset: usize,
    pub route: DisplayRoute,
    pub verified_binding: bool,
    pub conversational_capability: bool,
    pub physical_adapter: Option<&'static str>,
    pub physical_completion: &'static str,
    pub physical_access: &'static str,
    pub connectivity: &'static str,
}
impl RuntimeRoutes {
    pub fn new(ctx: &CogitatorContext) -> Self {
        Self {
            adapters: None,
            supported: ctx
                .connectors()
                .iter()
                .filter(|connector| {
                    connector
                        .capabilities
                        .event_kinds_emit
                        .iter()
                        .any(|kind| kind.as_str() == "chat.message")
                        && connector
                            .capabilities
                            .event_kinds_accept
                            .iter()
                            .any(|kind| kind.as_str() == "chat.reply")
                })
                .map(|connector| connector.id.to_string())
                .collect(),
        }
    }
    pub fn with_registry(ctx: &CogitatorContext, adapters: AdapterRegistry) -> Self {
        let mut routes = Self::new(ctx);
        routes.adapters = Some(adapters);
        routes
    }
    pub async fn discover(
        &self,
        service: &SessionService,
        actor: &str,
        offset: usize,
        limit: usize,
    ) -> MessagingResult<(Vec<RouteView>, bool, usize)> {
        let rows: Vec<(String, String)> = query_as("SELECT connector_id,channel_id FROM albert_private_session_routes WHERE user_id=? ORDER BY connector_id,channel_id LIMIT ? OFFSET ?")
            .bind(actor).bind((limit + 1) as i64).bind(offset as i64).fetch_all(service.database().pool()).await?;
        let more = rows.len() > limit;
        let next_scanned = offset + rows.len().min(limit);
        Ok((
            rows.into_iter()
                .take(limit)
                .enumerate()
                .filter(|(_, (connector, _))| self.supported.contains(connector))
                .map(|(index, (connector, channel))| RouteView {
                    next_offset: offset + index + 1,
                    physical_adapter: service
                        .delivery_adapters()
                        .get(&connector)
                        .map(AdapterKind::as_str),
                    physical_completion: match service.delivery_adapters().get(&connector) {
                        Some(AdapterKind::Console) => "stream_write_ack",
                        Some(AdapterKind::Telegram) => "chunk_observations_only",
                        None => "unsupported",
                    },
                    route: DisplayRoute { connector, channel },
                    verified_binding: true,
                    conversational_capability: true,
                    physical_access: "checked_by_adapter_at_send",
                    connectivity: "unknown",
                })
                .collect(),
            more,
            next_scanned,
        ))
    }
}
impl RouteGuard for RuntimeRoutes {
    fn check(&self, _: &str, route: &DisplayRoute) -> MessagingResult<()> {
        if !self.supported.contains(&route.connector)
            || self
                .adapters
                .as_ref()
                .is_some_and(|registry| registry.get(&route.connector).is_none())
        {
            return Err(MessagingError::Unavailable);
        }
        // The store rechecks the actor's verified route binding inside its tx.
        Ok(())
    }
}
