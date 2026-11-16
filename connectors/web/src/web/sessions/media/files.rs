use albert_core::{
    accounts::AccountRole, media::save_input_file, media::MediaInput, media::WorkspaceFile,
    service::session::web::WebActor, sessions::actor::ActorToolPolicy, sessions::SessionContext,
};
use tokio::task::spawn_blocking;

use super::super::SessionHost;
use crate::api::{errors::ApiError, errors::ApiResult};

impl SessionHost {
    pub(super) async fn prepare_workspace_files(
        &self,
        actor: &WebActor,
        input: &MediaInput,
        temporary: bool,
        context: Option<&SessionContext>,
        preparation: Option<impl Send + 'static>,
    ) -> ApiResult<Vec<WorkspaceFile>> {
        if temporary || actor.tools != ActorToolPolicy::ExistingHost {
            return Ok(Vec::new());
        }
        let files: Vec<_> = input
            .assets
            .iter()
            .zip(&input.blobs)
            .filter(|(_, blob)| !blob.is_image() && !blob.is_audio())
            .map(|(asset, blob)| (asset.id.clone(), blob.clone()))
            .collect();
        if files.is_empty() {
            return Ok(Vec::new());
        }
        let grant = self
            .runtime
            .sessions
            .accounts()
            .account(&actor.account_id)
            .await
            .map_err(|_| ApiError::forbidden())?;
        if grant.role != AccountRole::Owner || grant.user_id != actor.user_id.as_str() {
            return Err(ApiError::forbidden());
        }
        if let Some(context) = context {
            if !context
                .clone()
                .with_account(grant.clone())
                .authorized()
                .await
            {
                return Err(ApiError::forbidden());
            }
        }
        let root = self.runtime.base.code_workspace.clone();
        if let Some(context) = context {
            if !context.is_live() {
                return Err(ApiError::forbidden());
            }
            let ids = files.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>();
            self.runtime
                .sessions
                .media()
                .register_workspace_copies(context.session_id().as_str(), &ids, &root)
                .await
                .map_err(|_| {
                    ApiError::unavailable("Workspace preparation could not be recorded.")
                })?;
        }
        let prepared = spawn_blocking(move || {
            // Dropping the HTTP future does not stop a blocking worker. Keep its
            // session lease until it can no longer recreate managed copies.
            let _preparation = preparation;
            files
                .into_iter()
                .map(|(asset_id, blob)| {
                    let path = save_input_file(&root, &asset_id, blob.bytes()).ok();
                    WorkspaceFile { asset_id, path }
                })
                .collect()
        })
        .await
        .map_err(|_| {
            ApiError::unavailable("File preparation could not complete. Retry the same message.")
        })?;
        if !grant.valid().await {
            return Err(ApiError::forbidden());
        }
        Ok(prepared)
    }
}
