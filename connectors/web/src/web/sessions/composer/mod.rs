use std::time::Duration;

use albert_core::{
    cogitator::transcription_connector,
    composer::{CompactRequest, PreparedSkills, SkillSelection as CoreSelection},
    service::session::web::WebActor,
    sessions::actor::ActorToolPolicy,
};
use serde_json::to_vec;
use sha2::{Digest, Sha256};
use tokio::time::timeout;

use super::{
    send::{bound, namespace, request_id, response, InputExtras},
    SessionHost,
};
use crate::api::{
    errors::{ApiError, ApiResult},
    models::{CompactCreate, ComposerSkill, ComposerView, MessageReceipt, SkillSelection},
};

pub(super) fn selections(input: &[SkillSelection]) -> Vec<CoreSelection> {
    input
        .iter()
        .map(|skill| CoreSelection {
            id: skill.id.clone(),
            revision: skill.revision.clone(),
        })
        .collect()
}
impl SessionHost {
    pub(super) fn prepare_skills(
        &self,
        actor: &WebActor,
        input: &[CoreSelection],
    ) -> ApiResult<PreparedSkills> {
        if input.is_empty() {
            return Ok(PreparedSkills::default());
        }
        if actor.tools != ActorToolPolicy::ExistingHost {
            return Err(ApiError::forbidden());
        }
        self.runtime
            .current()
            .ok_or_else(|| ApiError::unavailable("Connect a model first."))?
            .panel_prepare_skills(input)
            .map_err(ApiError::conflict)
    }
    pub(super) async fn composer(&self, actor: &WebActor) -> ApiResult<ComposerView> {
        let owner = actor.tools == ActorToolPolicy::ExistingHost;
        let settings = self.runtime.sessions.settings();
        let skills = if owner {
            self.runtime
                .current()
                .map(|agent| agent.panel_composer_skills())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        Ok(ComposerView {
            speech_available: self
                .runtime
                .context()
                .as_ref()
                .and_then(transcription_connector)
                .is_some(),
            can_select_skills: owner,
            can_compact: true,
            can_restart: owner,
            can_reload_models: owner
                && self
                    .runtime
                    .current()
                    .is_some_and(|agent| agent.panel_model_reload_available()),
            max_file_bytes: settings.media.max_file_bytes,
            max_attachments: settings.media.max_attachments,
            max_selected_skills: settings.composer.max_selected_skills,
            recording_max_seconds: settings.composer.recording_max_seconds,
            skills: skills
                .into_iter()
                .map(|skill| ComposerSkill {
                    command: skill.command,
                    id: skill.id,
                    name: skill.name,
                    description: skill.description,
                    revision: skill.revision,
                })
                .collect(),
        })
    }
    pub(super) async fn compact(
        &self,
        actor: &WebActor,
        id: &str,
        input: CompactCreate,
    ) -> ApiResult<MessageReceipt> {
        let context = self.resource(actor, id).await?;
        let _gate = self.gate(context.session_id()).await;
        let request = request_id(&input.client_request_id)?;
        if input.instructions.len() > 16 * 1024 {
            return Err(ApiError::invalid("Compaction instructions are too long."));
        }
        let namespace = format!("{}/compact", namespace(actor));
        let digest = format!(
            "{:x}",
            Sha256::digest(to_vec(&("compact", &input.instructions)).expect("command serializes"))
        );
        let router = self.runtime.sessions.journal_router();
        if let Some(binding) = router
            .find_request(&namespace, &request)
            .await
            .map_err(|_| ApiError::unavailable("Command receipt could not be read."))?
        {
            let mut receipt = bound(binding, context.session_id(), Some(&digest))?;
            receipt.duplicate = true;
            return Ok(response(request, receipt));
        }
        if self.runtime.current().is_none() || !self.runtime.status().model_ready {
            return Err(ApiError::unavailable(
                "Connect a model before compacting this chat.",
            ));
        }
        let command = CompactRequest {
            instructions: input.instructions,
        };
        let receipt = self
            .dispatch_input(
                actor,
                &context,
                format!("/compact {}", command.instructions),
                &request,
                None,
                None,
                InputExtras {
                    digest: Some(digest.clone()),
                    compact: Some(command),
                    ..InputExtras::default()
                },
            )
            .await?;
        if let Ok(Ok(Ok(receipt))) = timeout(Duration::from_secs(10), receipt).await {
            return Ok(response(request, receipt));
        }
        if let Some(binding) = router
            .find_request(&namespace, &request)
            .await
            .map_err(|_| {
                ApiError::unavailable("Command acceptance is unknown. Retry the same command.")
            })?
        {
            return Ok(response(
                request,
                bound(binding, context.session_id(), Some(&digest))?,
            ));
        }
        Err(ApiError::unavailable(
            "Command acceptance is unknown. Retry with the same request ID.",
        ))
    }
}

#[cfg(test)]
mod tests {
    use albert_core::{
        composer::{message_digest, SkillSelection as CoreSelection},
        history::Turn,
        sessions::actor::ActorToolPolicy,
    };
    use octo_history::{AppendRequest, HistoryStore, SessionId};
    use serde_json::to_vec;
    use sha2::{Digest, Sha256};

    use super::{CompactCreate, SkillSelection};
    use crate::{
        api::models::MessageCreate,
        web::sessions::{fixture::fixture, send::namespace},
    };

    #[tokio::test]
    async fn admitted_skill_retries_recover_before_runtime_or_skill_catalog_lookup() {
        let (_dir, host, caller) = fixture().await;
        let actor = host.actor(&caller).await.unwrap();
        let session = host
            .runtime
            .sessions
            .web_create(&actor, "Receipt fixture", false)
            .await
            .unwrap();
        let context = host.resource(&actor, session.id.as_str()).await.unwrap();
        let request = SessionId::new().to_string();
        let selection = CoreSelection {
            id: "removed-skill".into(),
            revision: "old".into(),
        };
        let digest = message_digest("Accepted request", &[], &[selection.clone()]);
        host.runtime
            .sessions
            .journal_router()
            .append_journal(
                &context.root_history_key(),
                &AppendRequest {
                    namespace: namespace(&actor),
                    idempotency_key: request.clone(),
                    digest,
                    turns: vec![Turn::user("Accepted request")],
                },
            )
            .await
            .unwrap();
        let receipt = host
            .send(
                &actor,
                session.id.as_str(),
                MessageCreate {
                    client_message_id: request.clone(),
                    text: "Accepted request".into(),
                    digest: None,
                    attachments: vec![],
                    skills: vec![SkillSelection {
                        id: selection.id.clone(),
                        revision: selection.revision,
                    }],
                },
            )
            .await
            .unwrap();
        assert!(receipt.duplicate);
        assert!(host
            .send(
                &actor,
                session.id.as_str(),
                MessageCreate {
                    client_message_id: request,
                    text: "Accepted request".into(),
                    digest: None,
                    attachments: vec![],
                    skills: vec![SkillSelection {
                        id: selection.id,
                        revision: "new".into()
                    }]
                }
            )
            .await
            .is_err());
    }
    #[tokio::test]
    async fn compact_receipts_have_a_separate_namespace_and_no_model_is_invoked_on_replay() {
        let (_dir, host, caller) = fixture().await;
        let actor = host.actor(&caller).await.unwrap();
        let session = host
            .runtime
            .sessions
            .web_create(&actor, "Compact fixture", false)
            .await
            .unwrap();
        let context = host.resource(&actor, session.id.as_str()).await.unwrap();
        let request = SessionId::new().to_string();
        let digest = format!(
            "{:x}",
            Sha256::digest(to_vec(&("compact", "Keep decisions")).unwrap())
        );
        host.runtime
            .sessions
            .journal_router()
            .append_journal(
                &context.root_history_key(),
                &AppendRequest {
                    namespace: format!("{}/compact", namespace(&actor)),
                    idempotency_key: request.clone(),
                    digest,
                    turns: vec![Turn::user("/compact Keep decisions")],
                },
            )
            .await
            .unwrap();
        let receipt = host
            .compact(
                &actor,
                session.id.as_str(),
                CompactCreate {
                    client_request_id: request.clone(),
                    instructions: "Keep decisions".into(),
                },
            )
            .await
            .unwrap();
        assert!(receipt.duplicate);
        assert!(host
            .compact(
                &actor,
                session.id.as_str(),
                CompactCreate {
                    client_request_id: request,
                    instructions: "Changed".into()
                }
            )
            .await
            .is_err());
        let policy = host.composer(&actor).await.unwrap();
        assert!(!policy.speech_available);
        assert!(policy.skills.is_empty());
        assert!(policy.can_select_skills);
        let mut restricted = actor.clone();
        restricted.tools = ActorToolPolicy::ChatOnly;
        assert!(host
            .prepare_skills(
                &restricted,
                &[CoreSelection {
                    id: "forged".into(),
                    revision: "r".into()
                }]
            )
            .is_err());
    }
}
