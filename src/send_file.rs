//! `send_file` — a rig tool that sends a workspace file to the user in the current
//! chat. The model names a **workspace-relative path** (where the octo-code file
//! tools and `storage.checkout` write); the cogitator fills in the chat, so bytes
//! move **by reference**, never through the model. It emits a `chat.send_file`
//! envelope to the reply connector (e.g. Telegram) on the current channel — a
//! fire-and-forget publish, unlike the request/response `dispatch_to_connector`.
//!
//! Built per-turn (it captures the channel + reply target), so it is only present
//! when there is a user to send to (absent for silent routines).

use std::{convert::Infallible, sync::Arc};

use octo_core::{ChannelId, ConnectorId, Envelope, EventBus, EventKind, InProcessBus};
use rig::{completion::ToolDefinition, tool::Tool};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{info, warn};

const SEND_FILE: &str = "chat.send_file";

/// Emits `chat.send_file { path, filename? }` at `target` on `channel`.
#[derive(Clone)]
pub struct SendFileTool {
    bus: Arc<InProcessBus>,
    source: ConnectorId,
    target: ConnectorId,
    channel: String,
}

impl SendFileTool {
    pub fn new(
        bus: Arc<InProcessBus>,
        source: ConnectorId,
        target: ConnectorId,
        channel: impl Into<String>,
    ) -> Self {
        Self { bus, source, target, channel: channel.into() }
    }
}

impl Tool for SendFileTool {
    const NAME: &'static str = "send_file";
    type Error = Infallible;
    type Args = SendArgs;
    type Output = Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Send a file from your workspace to the user in this chat. Give `path` \
                          relative to your workspace — where the read/write/edit tools and \
                          storage.checkout put files. `filename` optionally overrides the shown \
                          name. The file is sent by reference; never paste its bytes."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "workspace-relative path to send" },
                    "filename": { "type": "string", "description": "optional display name" }
                },
                "required": ["path"]
            }),
        }
    }

    async fn call(&self, args: SendArgs) -> Result<Value, Infallible> {
        let mut payload = json!({ "path": args.path });
        if let Some(f) = &args.filename {
            payload["filename"] = json!(f);
        }
        let env = Envelope::new(self.source.clone(), EventKind::from_static(SEND_FILE), payload)
            .with_target(self.target.clone())
            .with_channel(ChannelId::new(self.channel.clone()));
        match self.bus.publish(env).await {
            Ok(()) => {
                info!(path = %args.path, target = %self.target, "send_file dispatched");
                Ok(json!({ "ok": true, "sent": args.path }))
            }
            Err(e) => {
                warn!(error = %e, "send_file publish failed");
                Ok(json!({ "ok": false, "error": e.to_string() }))
            }
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct SendArgs {
    pub path: String,
    #[serde(default)]
    pub filename: Option<String>,
}
