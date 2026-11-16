use albert_core::mcp::ServerPhase;
use serde_json::{from_value, to_value, Value};

use super::McpService;
use crate::api::{
    errors::ApiError, errors::ApiResult, models::McpServerConfig, models::McpServerList,
    models::McpServerView, models::McpToolList, models::McpToolQuery, models::McpToolView,
};
impl McpService {
    pub(super) fn view(&self) -> ApiResult<McpServerList> {
        let loaded = self.load()?;
        let items = loaded
            .config
            .servers
            .into_iter()
            .map(|server| {
                let status = self.runtime.mcp.status(&server.id);
                let phase = if !server.enabled {
                    "disabled"
                } else {
                    match status.as_ref().map(|state| state.phase) {
                        Some(ServerPhase::Connected) => "connected",
                        Some(ServerPhase::Error) => "error",
                        Some(ServerPhase::Disabled) => "disabled",
                        _ => "connecting",
                    }
                };
                let config: McpServerConfig =
                    from_value(to_value(server).map_err(|_| ApiError::internal())?)
                        .map_err(|_| ApiError::internal())?;
                Ok(McpServerView {
                    config,
                    phase: phase.into(),
                    tool_count: status.as_ref().map(|state| state.tool_count).unwrap_or(0),
                    error: status.and_then(|state| state.error),
                })
            })
            .collect::<ApiResult<Vec<_>>>()?;
        Ok(McpServerList {
            items,
            revision: loaded.revision,
        })
    }
    pub(super) fn tools(&self, id: &str, query: McpToolQuery) -> ApiResult<McpToolList> {
        let loaded = self.load()?;
        let server = loaded
            .config
            .servers
            .iter()
            .find(|server| server.id == id)
            .ok_or_else(ApiError::not_found)?;
        let offset = query.offset.unwrap_or(0);
        let limit = query.limit.unwrap_or(50);
        if offset > 10000 || !(1..=100).contains(&limit) {
            return Err(ApiError::invalid("Invalid tool page."));
        }
        let snapshot = self.runtime.mcp.tools(id);
        let tools = snapshot
            .as_ref()
            .map(|snapshot| snapshot.tools.as_slice())
            .unwrap_or_default();
        let missing_allowed = server
            .allowed_tools
            .iter()
            .filter(|name| !tools.iter().any(|tool| tool.name.as_ref() == name.as_str()))
            .cloned()
            .collect();
        let items = tools
            .iter()
            .skip(offset)
            .take(limit)
            .map(|tool| McpToolView {
                name: tool.name.to_string(),
                description: tool.description.as_deref().unwrap_or("").into(),
                allowed: server
                    .allowed_tools
                    .iter()
                    .any(|name| name == tool.name.as_ref()),
                schema: query
                    .include_schema
                    .unwrap_or(false)
                    .then(|| Value::Object((*tool.input_schema).clone())),
            })
            .collect();
        Ok(McpToolList {
            items,
            revision: loaded.revision,
            offset,
            next_offset: (offset.saturating_add(limit) < tools.len()).then(|| offset + limit),
            total: tools.len(),
            missing_allowed,
        })
    }
}
