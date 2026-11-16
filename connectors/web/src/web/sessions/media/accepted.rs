use albert_core::{
    media::decode, media::MediaInput, service::session::web::start::StartRecord,
    service::session::web::WebActor,
};
use octo_history::{HistoryStore, PageRequest, Role};

use super::{super::send::bound, super::send::namespace, error, SessionHost};
use crate::api::{errors::ApiError, errors::ApiResult};

impl SessionHost {
    /// Accepted input is immutable: recovery reloads its recorded preparation
    /// outcome, without writing files again or consulting newer workspace state.
    pub(in crate::web::sessions) async fn accepted_media(
        &self,
        actor: &WebActor,
        row: &StartRecord,
        text: &str,
        ids: &[String],
        digest: &str,
    ) -> ApiResult<MediaInput> {
        let context = self.resource(actor, &row.session).await?;
        let router = self.runtime.sessions.journal_router();
        let (binding, _lifecycle) = router
            .find_request_leased(&namespace(actor), &row.request)
            .await
            .map_err(|_| ApiError::not_found())?;
        let binding = binding.ok_or_else(ApiError::not_found)?;
        let receipt = bound(binding, context.session_id(), Some(digest))?;
        let [id] = receipt.record_ids.as_slice() else {
            return Err(ApiError::conflict(
                "The accepted input could not be recovered.",
            ));
        };
        let page = router
            .journal_page(
                &context.root_history_key(),
                PageRequest {
                    through_id: Some(*id),
                    limit: 1,
                    ..PageRequest::default()
                },
            )
            .await
            .map_err(|_| ApiError::not_found())?;
        let record = page
            .records
            .into_iter()
            .find(|record| record.id == *id && record.turn.role == Role::User)
            .ok_or_else(ApiError::not_found)?;
        let media = decode(&record.turn.content);
        let recorded_ids: Vec<_> = media
            .as_ref()
            .map(|media| {
                media
                    .attachments
                    .iter()
                    .map(|asset| asset.id.clone())
                    .collect()
            })
            .unwrap_or_default();
        if recorded_ids != ids {
            return Err(ApiError::conflict(
                "The accepted attachments do not match this request.",
            ));
        }
        let prepared = self
            .runtime
            .sessions
            .media()
            .prepare(&actor.account_id, Some(&row.session), ids)
            .await
            .map_err(error)?;
        let workspace_files = media.map(|media| media.workspace_files).unwrap_or_default();
        if workspace_files.iter().any(|file| {
            !ids.contains(&file.asset_id)
                || file
                    .path
                    .as_ref()
                    .is_some_and(|path| path != &format!("media-{}", file.asset_id))
        }) || (row.temporary && !workspace_files.is_empty())
        {
            return Err(ApiError::conflict(
                "The accepted file references could not be recovered.",
            ));
        }
        Ok(MediaInput {
            text: text.into(),
            assets: prepared.assets(),
            blobs: prepared.blobs(),
            workspace_files,
        })
    }
}
