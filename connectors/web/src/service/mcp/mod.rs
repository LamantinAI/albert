mod files;
mod views;

use std::sync::Arc;

use albert_core::runtime::AgentRuntime;
use tokio::sync::Mutex;

use crate::api::{
    errors::ApiError, errors::ApiResult, host::HostResponse, mcp::McpOperation,
    principal::Principal,
};

#[derive(Clone)]
pub(crate) struct McpService {
    runtime: Arc<AgentRuntime>,
    gate: Arc<Mutex<()>>,
}
impl McpService {
    pub fn new(runtime: Arc<AgentRuntime>, gate: Arc<Mutex<()>>) -> Self {
        Self { runtime, gate }
    }
    pub async fn operation(
        &self,
        caller: &Principal,
        operation: McpOperation,
    ) -> ApiResult<HostResponse> {
        caller.owner_permit()?;
        match operation {
            McpOperation::List => Ok(HostResponse::McpServers(self.view()?)),
            McpOperation::Tools { id, query } => {
                Ok(HostResponse::McpTools(self.tools(&id, query)?))
            }
            McpOperation::Reconnect { id } => {
                self.runtime.mcp.reconnect(&id).await.map_err(|_| {
                    ApiError::unavailable(
                        "The MCP server could not be reconnected. Review its settings.",
                    )
                })?;
                Ok(HostResponse::McpServers(self.view()?))
            }
            McpOperation::Refresh { id } => {
                self.runtime.mcp.refresh(&id).await.map_err(|_|ApiError::unavailable("The MCP tool contract could not be refreshed. Review the connection status."))?;
                Ok(HostResponse::McpServers(self.view()?))
            }
            mutation => {
                let _gate = self.gate.lock().await;
                let configuration = self.mutate(mutation)?;
                self.runtime
                    .mcp
                    .configure(configuration)
                    .await
                    .map_err(|_| {
                        ApiError::invalid("The saved MCP configuration could not be applied.")
                    })?;
                Ok(HostResponse::McpServers(self.view()?))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::McpService;
    use crate::{
        api::mcp::McpOperation, api::models::McpConnection, api::models::McpServerConfig,
        api::models::McpServerWrite, api::models::McpToolQuery, web::testing::fixture,
    };
    use axum::http::StatusCode;
    use std::{fs::read_to_string, sync::Arc};
    use tokio::sync::Mutex;
    fn config() -> McpServerConfig {
        McpServerConfig {
            id: "notes".into(),
            name: "Notes".into(),
            description: "Search notes".into(),
            enabled: false,
            connection: McpConnection::Http {
                url: "https://example.com/mcp".into(),
                token_env: Some("NOTES_TOKEN".into()),
            },
            allowed_tools: vec![],
        }
    }
    #[tokio::test]
    async fn configuration_bootstraps_without_secret_values_and_mutations_are_revisioned_and_lossless(
    ) {
        let (_dir, runtime, _auth, owner) = fixture().await;
        let service = McpService::new(runtime.clone(), Arc::new(Mutex::new(())));
        let first = service.view().unwrap();
        assert!(first.items.is_empty());
        let _ = service
            .operation(
                &owner,
                McpOperation::Create(McpServerWrite {
                    config: config(),
                    revision: first.revision.clone(),
                }),
            )
            .await
            .unwrap();
        let saved = service.view().unwrap();
        assert_eq!(saved.items[0].phase, "disabled");
        assert_eq!(saved.items[0].config.id, "notes");
        let path = runtime.base.mcp_registry.clone();
        let content = read_to_string(&path).unwrap();
        assert!(content.contains("NOTES_TOKEN"));
        assert!(!content.contains("Bearer "));
        let before = content;
        assert_eq!(
            service
                .operation(
                    &owner,
                    McpOperation::Delete {
                        id: "notes".into(),
                        revision: Some(first.revision)
                    }
                )
                .await
                .err()
                .unwrap()
                .status,
            StatusCode::PRECONDITION_FAILED
        );
        assert_eq!(read_to_string(&path).unwrap(), before);
        let tools = service.tools("notes", McpToolQuery::default()).unwrap();
        assert_eq!(tools.total, 0);
        let _ = service
            .operation(
                &owner,
                McpOperation::Delete {
                    id: "notes".into(),
                    revision: Some(saved.revision),
                },
            )
            .await
            .unwrap();
        assert!(service.view().unwrap().items.is_empty());
    }
    #[tokio::test]
    async fn invalid_transports_and_inline_credential_values_never_replace_saved_configuration() {
        let (_dir, runtime, _auth, owner) = fixture().await;
        let service = McpService::new(runtime, Arc::new(Mutex::new(())));
        let before = service.view().unwrap();
        let mut invalid = config();
        invalid.connection = McpConnection::Http {
            url: "https://user:password@example.com/mcp".into(),
            token_env: None,
        };
        assert!(service
            .operation(
                &owner,
                McpOperation::Create(McpServerWrite {
                    config: invalid,
                    revision: before.revision.clone()
                })
            )
            .await
            .is_err());
        assert_eq!(service.view().unwrap().revision, before.revision);
    }
}
