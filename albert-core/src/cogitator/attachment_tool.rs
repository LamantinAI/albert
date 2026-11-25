//! A workspace copy of an accepted attachment in the current conversation.
use std::{convert::Infallible, path::PathBuf, sync::Arc};

use rig::{completion::ToolDefinition, tool::Tool};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::task::spawn_blocking;

use crate::{
    history::sessions::Backend,
    media::{save_script_input_file, MediaError, MediaResult},
    service::session::SessionService,
    sessions::{actor::ActorToolPolicy, SessionContext},
};

pub(super) struct GetAttachment {
    pub(super) service: Arc<SessionService>,
    pub(super) session: SessionContext,
    pub(super) workspace: PathBuf,
    pub(super) allowed_ids: Option<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AttachmentArgs {
    attachment_id: String,
}

impl Tool for GetAttachment {
    const NAME: &'static str = "get_attachment";
    type Error = Infallible;
    type Args = AttachmentArgs;
    type Output = Value;

    async fn definition(&self, _: String) -> ToolDefinition {
        let description = if self.allowed_ids.is_some() {
            "Create a workspace copy of a file explicitly supplied to this run. Use its attachment ID. The path is relative to your run directory."
        } else {
            "Create a workspace copy of an attachment from this permanent conversation. Supply the ID shown with the attachment. The returned workspace-relative path can be opened by file tools and scripts."
        };
        ToolDefinition {
            name: Self::NAME.into(),
            description: description.into(),
            parameters: json!({"type":"object","properties":{"attachment_id":{"type":"string","maxLength":36}},"required":["attachment_id"],"additionalProperties":false}),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Value, Infallible> {
        if self
            .allowed_ids
            .as_ref()
            .is_some_and(|ids| !ids.iter().any(|id| *id == args.attachment_id))
        {
            return Ok(json!({"ok":false,"error":"This attachment was not supplied to this run."}));
        }
        let output = self.materialize(&args.attachment_id).await;
        Ok(match output {
            Ok(value) => value,
            Err(error) => json!({"ok":false, "error":error.to_string()}),
        })
    }
}

impl GetAttachment {
    async fn materialize(&self, id: &str) -> MediaResult<Value> {
        let session = self.session.session_id();
        let lease = self
            .service
            .journal_router()
            .handle(session)
            .await
            .read_owned()
            .await;
        if *lease != Backend::Durable
            || self.session.tool_policy() != ActorToolPolicy::ExistingHost
            || !self.session.authorized().await
        {
            return Err(MediaError::Scope);
        }
        let store = self.service.media();
        let (origin, asset) = self
            .service
            .resolve_attachment(&self.session, id, false)
            .await?;
        if asset.size > store.limits.max_file_bytes {
            return Err(MediaError::Limit);
        }
        let bytes = store
            .slice(origin.as_str(), id, false, 0, asset.size)
            .await?;
        // Forks own their workspace copies independently even when the configured
        // workspace is shared. The public attachment keeps its original ID.
        let copy_id = if *session == origin {
            asset.id.clone()
        } else {
            inherited_copy_id(session.as_str(), id)
        };
        store
            .register_workspace_copies(session.as_str(), &[copy_id.clone()], &self.workspace)
            .await?;
        let workspace = self.workspace.clone();
        let path = spawn_blocking(move || {
            // The worker can outlive a canceled turn. Keep the erasure fence
            // until the copy has either been published or failed.
            let _lease = lease;
            save_script_input_file(&workspace, &copy_id, &bytes)
        })
        .await
        .map_err(|_| MediaError::Workspace)??;
        if !self.session.authorized().await {
            return Err(MediaError::Scope);
        }
        Ok(
            json!({"ok":true,"attachment_id":asset.id,"path":path,"filename":asset.filename,
            "content_type":asset.content_type,"size":asset.size}),
        )
    }
}

// A deterministic UUIDv8 for materialization in the receiving session.
fn inherited_copy_id(session: &str, asset: &str) -> String {
    let hash = format!(
        "{:x}",
        Sha256::digest(format!("attachment-copy/{session}/{asset}"))
    );
    format!(
        "{}-{}-8{}-8{}-{}",
        &hash[..8],
        &hash[8..12],
        &hash[13..16],
        &hash[17..20],
        &hash[20..32]
    )
}

#[cfg(test)]
mod tests {
    use std::{
        fs::{create_dir, read},
        sync::Arc,
    };

    use octo_history::{AppendRequest, HistoryStore, SessionId, SessionUpdate, Turn};
    use rig::tool::Tool;

    use super::{AttachmentArgs, GetAttachment};
    use crate::{
        media::{encode, Asset},
        service::session::web::{tests::fixture, WebActor},
        service::session::SessionService,
        sessions::{actor::ActorToolPolicy, error::SessionError, SessionContext},
    };

    #[tokio::test]
    async fn opens_only_a_durable_attachment_in_the_current_conversation() {
        let (dir, service, chat_only, owner) = fixture().await;
        let service = Arc::new(service);
        let workspace = dir.path().join("workspace");
        create_dir(&workspace).unwrap();
        let chat = service.web_create(&owner, "Files", false).await.unwrap();
        let other = service
            .web_create(&owner, "Other files", false)
            .await
            .unwrap();
        let temporary = service.web_create(&owner, "Transient", true).await.unwrap();
        let context = service.web_context(&owner, &chat.id).await.unwrap();
        let source = b"\x89PNG\r\n\x1a\nfixture";
        let asset = service
            .media()
            .stage(
                &owner.account_id,
                source.to_vec(),
                "image/png",
                "fixture.png",
            )
            .await
            .unwrap();
        service
            .journal_router()
            .append_journal(
                &context.root_history_key(),
                &AppendRequest {
                    namespace: "attachment-tool-test".into(),
                    idempotency_key: SessionId::new().to_string(),
                    digest: asset.id.clone(),
                    turns: vec![Turn::user(encode(String::new(), vec![asset.clone()]))],
                },
            )
            .await
            .unwrap();
        let mut tool = GetAttachment {
            service: service.clone(),
            session: context,
            workspace: workspace.clone(),
            allowed_ids: None,
        };
        let call = || AttachmentArgs {
            attachment_id: asset.id.clone(),
        };
        let result = tool.call(call()).await.unwrap();
        assert_eq!(result["ok"], true);
        assert_eq!(result["filename"], "fixture.png");
        assert_eq!(
            read(workspace.join(result["path"].as_str().unwrap())).unwrap(),
            source
        );
        assert_eq!(tool.call(call()).await.unwrap(), result);
        tool.session = service.web_context(&owner, &other.id).await.unwrap();
        assert_eq!(tool.call(call()).await.unwrap()["ok"], false);
        tool.session = service.web_context(&owner, &temporary.id).await.unwrap();
        assert_eq!(tool.call(call()).await.unwrap()["ok"], false);
        let restricted = service
            .web_create(&chat_only, "Restricted", false)
            .await
            .unwrap();
        tool.session = service
            .web_context(&chat_only, &restricted.id)
            .await
            .unwrap();
        assert_eq!(tool.session.tool_policy(), ActorToolPolicy::ChatOnly);
        assert_eq!(tool.call(call()).await.unwrap()["ok"], false);
    }
    async fn accepted_fixture(
        service: &SessionService,
        actor: &WebActor,
        context: &SessionContext,
        bytes: &[u8],
    ) -> Asset {
        let asset = service
            .media()
            .stage(
                &actor.account_id,
                bytes.to_vec(),
                "application/octet-stream",
                "fixture.bin",
            )
            .await
            .unwrap();
        service
            .journal_router()
            .append_journal(
                &context.root_history_key(),
                &AppendRequest {
                    namespace: "fork-media-test".into(),
                    idempotency_key: asset.id.clone(),
                    digest: asset.id.clone(),
                    turns: vec![Turn::user(encode(String::new(), vec![asset.clone()]))],
                },
            )
            .await
            .unwrap();
        asset
    }

    #[tokio::test]
    async fn fork_materializes_only_inherited_assets_and_owns_its_workspace_copy() {
        let (dir, service, _, owner) = fixture().await;
        let service = Arc::new(service);
        let workspace = dir.path().join("workspace");
        create_dir(&workspace).unwrap();
        let parent = service.web_create(&owner, "Parent", false).await.unwrap();
        let original = service.web_context(&owner, &parent.id).await.unwrap();
        let early = accepted_fixture(&service, &owner, &original, b"before branch").await;
        let mut tx = service.history().begin_journal().await.unwrap();
        let fork = service
            .history()
            .fork_session_in(&mut tx, &parent.id, None, "Fork".into())
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let late = accepted_fixture(&service, &owner, &original, b"after branch").await;
        let fork_context = service.web_context(&owner, &fork.id).await.unwrap();
        let mut tool = GetAttachment {
            service: service.clone(),
            session: original,
            workspace: workspace.clone(),
            allowed_ids: None,
        };
        let parent_copy = tool
            .call(AttachmentArgs {
                attachment_id: early.id.clone(),
            })
            .await
            .unwrap();
        tool.session = fork_context;
        let fork_copy = tool
            .call(AttachmentArgs {
                attachment_id: early.id.clone(),
            })
            .await
            .unwrap();
        assert_eq!(parent_copy["ok"], true);
        assert_eq!(fork_copy["ok"], true);
        assert_ne!(parent_copy["path"], fork_copy["path"]);
        assert_eq!(
            read(workspace.join(fork_copy["path"].as_str().unwrap())).unwrap(),
            b"before branch"
        );
        assert_eq!(
            tool.call(AttachmentArgs {
                attachment_id: late.id
            })
            .await
            .unwrap()["ok"],
            false
        );
        let archived = service
            .web_update(
                &owner,
                &parent.id,
                SessionUpdate {
                    expected_revision: parent.revision,
                    title: None,
                    archived: Some(true),
                },
            )
            .await
            .unwrap();
        assert!(
            matches!(service.web_delete(&owner, &parent.id, archived.revision, &workspace).await,
            Err(SessionError::ForkDependents(ids)) if ids == vec![fork.id.clone()])
        );
        assert_eq!(
            tool.call(AttachmentArgs {
                attachment_id: early.id.clone()
            })
            .await
            .unwrap()["ok"],
            true
        );
        service
            .web_delete(&owner, &fork.id, fork.revision, &workspace)
            .await
            .unwrap();
        let copy = service.erasure_files(&fork.id).await.unwrap().unwrap();
        assert_eq!(copy.assets.len(), 1);
        assert_eq!(
            format!("media-{}", copy.assets[0].0),
            fork_copy["path"].as_str().unwrap()
        );
        assert_eq!(
            service
                .media()
                .slice(parent.id.as_str(), &early.id, false, 0, early.size)
                .await
                .unwrap(),
            b"before branch"
        );
    }
}
