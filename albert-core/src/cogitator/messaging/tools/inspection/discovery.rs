use super::{size, summary};
use crate::cogitator::messaging::tools::SessionMessagingTool;
use crate::{
    cogitator::messaging::routes::RuntimeRoutes, messaging::MessagingError,
    messaging::MessagingResult, service::session::SessionService,
};
use octo_history::PageRequest;
use serde_json::{json, to_value, Value};
impl SessionMessagingTool {
    pub(in crate::cogitator::messaging::tools) async fn discover_bounded(
        &self,
        service: &SessionService,
        guard: &RuntimeRoutes,
        limit: usize,
        before: Option<i64>,
        through: Option<i64>,
        route_offset: usize,
    ) -> MessagingResult<Value> {
        if limit == 0
            || limit > service.settings().limits.max_page_size as usize
            || route_offset > 1_000_000
        {
            return Err(MessagingError::Invalid("invalid discovery page"));
        }
        let cap = self.runtime.settings.inspection_output_bytes;
        let (routes, routes_more, scanned_next) = guard
            .discover(
                service,
                self.source.context().private_user_id()?.as_str(),
                route_offset,
                limit,
            )
            .await?;
        let mut result = json!({"sessions":{"sessions":[],"through_id":through,"next_before":before,"has_more":false},
            "routes":[],"routes_next_offset":null,"routes_has_more":routes_more,"delegatable_connectors":[],"truncated":false,
            "note":"Private metadata only. Connectivity is unknown; actual physical access is checked by the adapter. Use connector_search for further connector contracts."});
        let mut route_next = route_offset;
        for route in routes {
            let next = route.next_offset;
            result["routes"]
                .as_array_mut()
                .unwrap()
                .push(to_value(route)?);
            if size(&result) > cap / 2 {
                result["routes"].as_array_mut().unwrap().pop();
                if result["routes"].as_array().unwrap().is_empty() {
                    return Err(MessagingError::Invalid("next route exceeds inspection_output_bytes; increase the configured inspection budget"));
                }
                result["truncated"] = true.into();
                result["routes_has_more"] = true.into();
                break;
            }
            route_next = next;
        }
        if result["truncated"] != true {
            route_next = scanned_next;
        }
        if result["routes_has_more"] == true {
            result["routes_next_offset"] = route_next.into();
        }
        for grant in self.available_grants() {
            result["delegatable_connectors"]
                .as_array_mut()
                .unwrap()
                .push(to_value(grant)?);
            if size(&result) > cap * 2 / 3 {
                result["delegatable_connectors"]
                    .as_array_mut()
                    .unwrap()
                    .pop();
                result["truncated"] = true.into();
                result["connector_contracts_truncated"] = true.into();
                break;
            }
        }
        let mut before = before;
        let mut through = through;
        for _ in 0..limit {
            let page = service
                .list_private(
                    self.source.context(),
                    PageRequest {
                        limit: 1,
                        before_id: before,
                        through_id: through,
                    },
                )
                .await?;
            through = Some(page.through_id);
            result["sessions"]["through_id"] = page.through_id.into();
            let Some(session) = page.sessions.first() else {
                result["sessions"]["has_more"] = false.into();
                break;
            };
            result["sessions"]["sessions"]
                .as_array_mut()
                .unwrap()
                .push(summary(session));
            if size(&result) > cap {
                result["sessions"]["sessions"].as_array_mut().unwrap().pop();
                if result["sessions"]["sessions"]
                    .as_array()
                    .unwrap()
                    .is_empty()
                {
                    return Err(MessagingError::Invalid("session metadata exceeds inspection_output_bytes; increase the configured inspection budget"));
                }
                result["truncated"] = true.into();
                result["sessions"]["has_more"] = true.into();
                result["sessions"]["next_before"] = to_value(before)?;
                break;
            }
            before = page.next_before;
            result["sessions"]["next_before"] = to_value(before)?;
            result["sessions"]["has_more"] = page.has_more.into();
            if !page.has_more {
                break;
            }
        }
        if size(&result) > cap {
            return Err(MessagingError::Limit);
        }
        Ok(result)
    }
}
