//! Preserve confirmed native file outputs in the same canonical conversation.
use std::{convert::Infallible, path::PathBuf, sync::Arc};

use octo_history::{AppendRequest, HistoryStore, SessionId, Turn};
use octo_rig::{SendFileArgs, SendFileTool};
use rig::{completion::ToolDefinition, tool::Tool};
use serde_json::{json, Value};
use tokio::task::spawn_blocking;

use crate::{
    media::encode, media::read_workspace_file, service::session::SessionService,
    sessions::SessionContext,
};

#[derive(Clone)]
pub(super) struct MediaDelivery {
    inner: SendFileTool,
    service: Option<Arc<SessionService>>,
    session: Option<SessionContext>,
    workspace: PathBuf,
    web: bool,
}
impl MediaDelivery {
    pub(super) fn new(
        inner: SendFileTool,
        service: Option<Arc<SessionService>>,
        session: Option<SessionContext>,
        workspace: PathBuf,
        web: bool,
    ) -> Self {
        Self {
            inner,
            service,
            session,
            workspace,
            web,
        }
    }
}
impl Tool for MediaDelivery {
    const NAME: &'static str = "send_file";
    type Error = Infallible;
    type Args = SendFileArgs;
    type Output = Value;
    async fn definition(&self, prompt: String) -> ToolDefinition {
        self.inner.definition(prompt).await
    }
    async fn call(&self, args: Self::Args) -> Result<Value, Self::Error> {
        let (Some(service), Some(session)) = (&self.service, &self.session) else {
            return self.inner.call(args).await;
        };
        if !session.authorized().await {
            return Ok(
                json!({"ok":false,"status":"failed","error":"Conversation access has ended."}),
            );
        }
        if self.web {
            return self.inner.call(args).await;
        }
        let root = self.workspace.clone();
        let path = args.path.clone();
        let limit = service.media().limits.max_file_bytes;
        let bytes = spawn_blocking(move || read_workspace_file(&root, &path, limit)).await;
        let asset = match bytes {
            Ok(Ok(bytes)) => service
                .media()
                .stage(
                    session.session_id().as_str(),
                    bytes,
                    "application/octet-stream",
                    args.filename.as_deref().unwrap_or(&args.path),
                )
                .await
                .ok(),
            _ => None,
        };
        let mut result = self.inner.call(args).await?;
        if let Some(asset) = asset {
            if result["status"] == "sent" {
                let request = AppendRequest {
                    namespace: "albert.native.media".into(),
                    idempotency_key: SessionId::new().to_string(),
                    digest: asset.id.clone(),
                    turns: vec![Turn::assistant(encode(String::new(), vec![asset.clone()]))],
                };
                if service
                    .journal_router()
                    .append_journal(&session.root_history_key(), &request)
                    .await
                    .is_err()
                {
                    result["history_warning"]=json!("Delivery was confirmed, but its media preview could not be retained. Do not repeat the delivery.");
                    let _ = service
                        .media()
                        .remove_staged(session.session_id().as_str(), &asset.id)
                        .await;
                }
            } else {
                let _ = service
                    .media()
                    .remove_staged(session.session_id().as_str(), &asset.id)
                    .await;
            }
        }
        Ok(result)
    }
}
