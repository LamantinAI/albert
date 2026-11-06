use std::path::Path;

use albert_core::{
    media::encode, media::read_workspace_file, sessions::transport::OutputScope,
    sessions::SessionContext,
};
use octo_core::{ConnectorContext, ConnectorId, Envelope, EventKind};
use octo_history::{AppendRequest, HistoryStore, Turn};
use serde_json::{json, Value};
use tokio::task::spawn_blocking;

use super::SessionHost;

impl SessionHost {
    pub async fn deliver_file(&self, io: &ConnectorContext, event: &Envelope) {
        if event.kind.as_str() != "chat.send_file"
            || event.target.as_ref().is_none_or(|t| t.as_str() != "web")
        {
            return;
        }
        let outcome = self.store_file(event).await;
        let result = match outcome {
            Ok(ids) => json!({"ok":true,"status":"sent","message_ids":ids}),
            Err(()) => {
                json!({"ok":false,"status":"failed","error":"The file could not be delivered to this conversation. Check that it exists in the workspace, fits the media limits and the session is still active."})
            }
        };
        let response = Envelope::new(
            ConnectorId::new("web"),
            EventKind::new("chat.send_file.result"),
            result,
        )
        .with_target(event.source.clone())
        .with_correlation(event.id);
        let _ = io.publish(response).await;
    }
    async fn store_file(&self, event: &Envelope) -> Result<Vec<String>, ()> {
        let context = event.context.get::<SessionContext>().ok_or(())?;
        let scope = event.context.get::<OutputScope>().ok_or(())?;
        if !context.is_private()
            || !context.authorized().await
            || context.reply_route().connector() != "web"
            || event.channel.as_ref().map(|c| c.as_str()) != Some(context.reply_route().channel())
            || scope.scope.key != context.root_history_key()
        {
            return Err(());
        }
        self.runtime
            .sessions
            .validate_context(context)
            .await
            .map_err(|_| ())?;
        if let Some(binding) = self
            .runtime
            .sessions
            .journal_router()
            .find_request("albert.web.media", &event.id.to_string())
            .await
            .map_err(|_| ())?
        {
            if binding.journal.session_id() != context.session_id() {
                return Err(());
            }
            return Ok(binding
                .receipt
                .record_ids
                .iter()
                .map(ToString::to_string)
                .collect());
        }
        let payload = event.payload_as::<Value>().ok_or(())?;
        let path = payload["path"].as_str().ok_or(())?.to_owned();
        let filename = payload["filename"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| {
                Path::new(&path)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("attachment")
                    .into()
            });
        let root = self.runtime.base.code_workspace.clone();
        let limit = self.runtime.sessions.media().limits.max_file_bytes;
        let bytes = spawn_blocking(move || read_workspace_file(&root, &path, limit))
            .await
            .map_err(|_| ())?
            .map_err(|_| ())?;
        let asset = self
            .runtime
            .sessions
            .media()
            .stage(
                context.session_id().as_str(),
                bytes,
                "application/octet-stream",
                &filename,
            )
            .await
            .map_err(|_| ())?;
        if !context.authorized().await {
            let _ = self
                .runtime
                .sessions
                .media()
                .remove_staged(context.session_id().as_str(), &asset.id)
                .await;
            return Err(());
        }
        let request = AppendRequest {
            namespace: "albert.web.media".into(),
            idempotency_key: event.id.to_string(),
            digest: event.id.to_string(),
            turns: vec![Turn::assistant(encode(String::new(), vec![asset]))],
        };
        let receipt = self
            .runtime
            .sessions
            .journal_router()
            .append_journal(&context.root_history_key(), &request)
            .await
            .map_err(|_| ())?;
        self.hub.changed(
            context.session_id(),
            context.private_user_id().map_err(|_| ())?.as_str(),
            &self.runtime.status().runtime_epoch,
            "changed",
            None,
        );
        Ok(receipt.record_ids.iter().map(ToString::to_string).collect())
    }
}

#[cfg(test)]
mod tests {
    use std::fs::{create_dir_all, write};

    use albert_core::{media::decode, sessions::output::output_scope};
    use octo_core::{ChannelId, ConnectorId, Envelope, EventKind};
    use octo_history::{HistoryStore, PageRequest};
    use serde_json::json;

    use super::super::fixture::fixture;

    #[tokio::test]
    async fn web_file_output_is_snapshotted_once_and_closed_sessions_are_fenced() {
        let (_dir, host, caller) = fixture().await;
        let actor = host.actor(&caller).await.unwrap();
        let session = host
            .runtime
            .sessions
            .web_create(&actor, "Output", true)
            .await
            .unwrap();
        let grant = host
            .runtime
            .sessions
            .accounts()
            .account(&actor.account_id)
            .await
            .unwrap();
        let context = host
            .resource(&actor, session.id.as_str())
            .await
            .unwrap()
            .with_account(grant);
        create_dir_all(&host.runtime.base.code_workspace).unwrap();
        write(
            host.runtime.base.code_workspace.join("frame.png"),
            b"\x89PNG\r\n\x1a\nsynthetic",
        )
        .unwrap();
        let event = Envelope::new(
            ConnectorId::new("albert"),
            EventKind::new("chat.send_file"),
            json!({"path":"frame.png"}),
        )
        .with_target(ConnectorId::new("web"))
        .with_channel(ChannelId::new(context.reply_route().channel()))
        .with_context(context.clone())
        .with_context(output_scope(&context, None));
        let first = host.store_file(&event).await.unwrap();
        let second = host.store_file(&event).await.unwrap();
        assert_eq!(first, second);
        write(
            host.runtime.base.code_workspace.join("frame.png"),
            b"changed",
        )
        .unwrap();
        let page = host
            .runtime
            .sessions
            .journal_router()
            .journal_page(&context.root_history_key(), PageRequest::default())
            .await
            .unwrap();
        assert_eq!(page.records.len(), 1);
        let asset = decode(&page.records[0].turn.content)
            .unwrap()
            .attachments
            .remove(0);
        assert_eq!(
            host.runtime
                .sessions
                .media()
                .slice(session.id.as_str(), &asset.id, true, 0, asset.size)
                .await
                .unwrap(),
            b"\x89PNG\r\n\x1a\nsynthetic"
        );
        host.runtime
            .sessions
            .web_end(&actor, &session.id, session.revision)
            .await
            .unwrap();
        assert!(host.store_file(&event).await.is_err());
    }
}
