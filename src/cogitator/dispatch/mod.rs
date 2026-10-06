//! Albert's request/reply tool must not wait for fire-and-forget file delivery.
use std::convert::Infallible;

use octo_rig::{DispatchArgs, OctoDispatchTool};
use rig::{completion::ToolDefinition, tool::Tool};
use serde_json::{json, Value};

#[derive(Clone)]
pub(super) struct AgentDispatch(pub OctoDispatchTool);

impl Tool for AgentDispatch {
    const NAME: &'static str = OctoDispatchTool::NAME;
    type Error = Infallible;
    type Args = DispatchArgs;
    type Output = Value;

    async fn definition(&self, prompt: String) -> ToolDefinition {
        let mut definition = self.0.definition(prompt).await;
        definition.description.push_str(" For sending workspace files to this chat, use the separate send_file tool. Never dispatch chat.send_file: it is a fire-and-forget event, not a request/reply connector command.");
        definition
    }

    async fn call(&self, args: DispatchArgs) -> Result<Value, Infallible> {
        if args.kind == "chat.send_file" {
            return Ok(
                json!({"ok":false,"error":"No file was sent. Use the separate send_file tool with path and optional filename. chat.send_file cannot be dispatched: it has no correlated reply and dispatch does not bind the destination chat."}),
            );
        }
        self.0.call(args).await
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use octo_core::{ConnectorId, InProcessBus};
    use tokio::time::timeout;

    use super::*;

    #[tokio::test]
    async fn misrouted_file_send_returns_immediately_without_a_connector() {
        let tool = AgentDispatch(OctoDispatchTool::new(
            Arc::new(InProcessBus::new(8)),
            ConnectorId::new("agent"),
            "",
        ));
        let result = timeout(
            Duration::from_millis(500),
            tool.call(DispatchArgs {
                target: "telegram".into(),
                kind: "chat.send_file".into(),
                payload: json!({"path":"image.png"}),
            }),
        )
        .await
        .expect("must not await a response to a fire-and-forget event")
        .unwrap();
        assert_eq!(result["ok"], false);
        assert!(result["error"]
            .as_str()
            .unwrap()
            .contains("No file was sent"));
        assert!(result["error"]
            .as_str()
            .unwrap()
            .contains("separate send_file tool"));
    }
}
