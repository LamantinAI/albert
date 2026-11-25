//! Explicit per-run attachment grants from the parent's authorized conversation.
use std::{collections::HashSet, path::PathBuf};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use rig::{completion::Message, message::UserContent, OneOrMany};

use super::super::{attachment_tool::GetAttachment, context::media_type, AlbertCogitator};
use crate::{
    history::sessions::Backend,
    media::{attachment_note, MediaError},
    models::Snapshot,
    sessions::actor::ActorToolPolicy,
    subagents::{Run, SubagentTool, Task},
};

#[derive(Default)]
pub(super) struct ChildAttachments {
    prompt_parts: Vec<UserContent>,
    pub(super) vision: bool,
    needs_file_tool: bool,
    durable: bool,
}

impl ChildAttachments {
    pub(super) fn validate_models(&self, snapshot: &Snapshot, task: &Task) -> Result<(), String> {
        if task.attachment_ids.is_empty() {
            return Ok(());
        }
        let tools = self.requires_tools() || !task.tools.is_empty() || !task.connectors.is_empty();
        if !snapshot
            .config
            .models
            .iter()
            .any(|model| (!self.vision || model.vision) && (!tools || model.tools))
        {
            return Err("No model in the assigned child pool supports the supplied attachments and requested tools. Select an eligible model or role from subagent capabilities.".into());
        }
        Ok(())
    }

    pub(super) fn requires_tools(&self) -> bool {
        self.needs_file_tool
    }

    pub(super) fn prompt(self, task: &Task) -> Message {
        let text = format!(
            "Task:\n{}\n\nExplicit context:\n{}",
            task.task, task.context
        );
        let mut parts = vec![UserContent::text(text)];
        parts.extend(self.prompt_parts);
        Message::User {
            content: OneOrMany::many(parts).expect("child prompt includes the task"),
        }
    }

    pub(super) fn workspace_tool(
        &self,
        host: &AlbertCogitator,
        run: &Run,
        task: &Task,
        workspace: PathBuf,
    ) -> Option<GetAttachment> {
        if !self.durable || task.attachment_ids.is_empty() {
            return None;
        }
        Some(GetAttachment {
            service: host.session_service.clone()?,
            session: run.session.clone()?,
            workspace,
            allowed_ids: Some(task.attachment_ids.clone()),
        })
    }
}

impl AlbertCogitator {
    pub(super) async fn prepare_child_attachments(
        &self,
        tool: &SubagentTool,
        ids: &[String],
    ) -> Result<ChildAttachments, String> {
        if ids.is_empty() {
            return Ok(ChildAttachments::default());
        }
        let resolve = async {
            let service = self.session_service.as_ref().ok_or(MediaError::Scope)?;
            let context = tool.session.as_ref().ok_or(MediaError::Scope)?;
            if !tool.owner || context.tool_policy() != ActorToolPolicy::ExistingHost {
                return Err(MediaError::Scope);
            }
            let id = context.session_id();
            let lease = service.journal_router().handle(id).await.read_owned().await;
            if *lease == Backend::Closed || !context.authorized().await {
                return Err(MediaError::Scope);
            }
            let temporary = *lease == Backend::Temporary;
            let store = service.media();
            let mut seen = HashSet::new();
            if ids.len() > store.limits.max_attachments {
                return Err(MediaError::Limit);
            }
            let mut total = 0usize;
            let mut attachments = ChildAttachments {
                durable: !temporary,
                ..Default::default()
            };
            for asset_id in ids {
                if !seen.insert(asset_id) {
                    return Err(MediaError::Scope);
                }
                let asset = store.metadata(id.as_str(), asset_id, temporary).await?;
                total = total
                    .checked_add(asset.size)
                    .filter(|size| *size <= store.limits.max_file_bytes)
                    .ok_or(MediaError::Limit)?;
                attachments
                    .prompt_parts
                    .push(UserContent::text(attachment_note(&asset)));
                if asset.content_type.starts_with("image/") {
                    let bytes = store
                        .slice(id.as_str(), asset_id, temporary, 0, asset.size)
                        .await?;
                    attachments.prompt_parts.push(UserContent::image_base64(
                        STANDARD.encode(bytes),
                        Some(media_type(&asset.content_type)),
                        None,
                    ));
                    attachments.vision = true;
                } else if temporary {
                    return Err(MediaError::Scope);
                } else {
                    attachments.needs_file_tool = true;
                }
            }
            if !context.authorized().await {
                return Err(MediaError::Scope);
            }
            Ok(attachments)
        };
        resolve
            .await
            .map_err(|error| format!("Child attachment grant rejected: {error}"))
    }
}
