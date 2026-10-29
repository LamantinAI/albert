use std::{sync::Arc, time::Duration};

use rand::random;
use sqlx::{query, query_as};
use tokio::{spawn, time::sleep};

use super::ControlService;
use crate::{
    api::{
        errors::{ApiError, ApiResult},
        models::{ApplyRequest, ApplyView},
        principal::OwnerPermit,
    },
    config::control::ControlSettings,
    service::panel_models::files::hash,
};

type Stored = (
    String,
    String,
    String,
    String,
    String,
    String,
    Option<String>,
);

impl ControlService {
    pub(super) async fn application(&self, permit: &OwnerPermit, id: &str) -> ApiResult<ApplyView> {
        let pool = self.runtime.sessions.database().pool();
        let record:Option<Stored>=query_as("SELECT id,request_hash,revision,state,runtime_epoch,message,document_id FROM albert_control_applications WHERE id=? AND account_id=?")
            .bind(id).bind(permit.principal().account_id()).fetch_optional(pool).await.map_err(|_|ApiError::internal())?;
        let Some((id, _, revision, mut state, mut epoch, mut message, document_id)) = record else {
            return Err(ApiError::not_found());
        };
        let runtime = self.runtime.status();
        if state == "restarting" && epoch != runtime.runtime_epoch && runtime.phase != "starting" {
            let problem = runtime
                .error
                .or_else(|| self.runtime.connector_error(document_id.as_deref()));
            state = if problem.is_some() {
                "failed"
            } else {
                "applied"
            }
            .into();
            message =
                problem.unwrap_or_else(|| "Runtime restarted with the saved configuration.".into());
            epoch = runtime.runtime_epoch;
            query("UPDATE albert_control_applications SET state=?,runtime_epoch=?,message=? WHERE id=?")
                .bind(&state).bind(&epoch).bind(&message).bind(&id).execute(pool).await.map_err(|_|ApiError::internal())?;
        }
        Ok(ApplyView {
            id,
            revision,
            state,
            runtime_epoch: epoch,
            message,
        })
    }

    pub(super) async fn apply(
        &self,
        permit: &OwnerPermit,
        input: ApplyRequest,
    ) -> ApiResult<ApplyView> {
        if !["runtime", "process"].contains(&input.target.as_str())
            || input.client_request_id.is_empty()
            || input.client_request_id.len() > 128
        {
            return Err(ApiError::invalid(
                "Choose runtime or process and provide a client request ID.",
            ));
        }
        let request_hash = hash(&[
            input.target.as_bytes(),
            input.document_id.as_deref().unwrap_or("").as_bytes(),
            input.revision.as_deref().unwrap_or("").as_bytes(),
        ]);
        let _gate = self.gate.lock().await;
        let pool = self.runtime.sessions.database().pool();
        let previous:Option<(String,String)>=query_as("SELECT id,request_hash FROM albert_control_applications WHERE account_id=? AND client_request_id=?")
            .bind(permit.principal().account_id()).bind(&input.client_request_id).fetch_optional(pool).await.map_err(|_|ApiError::internal())?;
        if let Some((id, expected)) = previous {
            if expected != request_hash {
                return Err(ApiError::conflict(
                    "This application request ID was used for different settings.",
                ));
            }
            return self.application(permit, &id).await;
        }
        let revision = self.revision()?;
        let expected = if let Some(id) = &input.document_id {
            self.document(id)?.revision
        } else {
            revision.clone()
        };
        if input.revision.as_deref() != Some(expected.as_str()) {
            return Err(ApiError::conflict(
                "Saved settings changed. Reload before applying.",
            ));
        }
        let id = format!("application-{:032x}", random::<u128>());
        let epoch = self.runtime.status().runtime_epoch;
        query("INSERT INTO albert_control_applications(id,account_id,client_request_id,request_hash,target,document_id,revision,state,message,runtime_epoch) VALUES (?,?,?,?,?,?,?,'pending','Applying saved settings',?)")
            .bind(&id).bind(permit.principal().account_id()).bind(&input.client_request_id).bind(request_hash).bind(&input.target).bind(&input.document_id).bind(&revision).bind(&epoch)
            .execute(pool).await.map_err(|_|ApiError::internal())?;
        let needs_process = input
            .document_id
            .as_deref()
            .is_some_and(|id| id == "albert" || id == "connectors" || id.starts_with("connector:"));
        let (state, message) = if input.target == "process"
            && ControlSettings::from_path(&self.runtime.path).is_err()
        {
            (
                "failed".to_owned(),
                "Invalid control-plane settings. Correct them before restarting.".to_owned(),
            )
        } else if input.target == "process" {
            let runtime = Arc::clone(&self.runtime);
            spawn(async move {
                sleep(Duration::from_millis(300)).await;
                let _ = runtime.request_restart();
            });
            (
                "restarting".to_owned(),
                "Restart requested. The panel will reconnect automatically.".to_owned(),
            )
        } else if needs_process {
            (
                "restart_required".to_owned(),
                "Saved. Restart the runtime to apply this configuration document.".to_owned(),
            )
        } else {
            match self.runtime.activate().await {
                Ok(())=>("applied".to_owned(),"Model, prompt and skill changes applied. Running turns retain their model snapshot.".to_owned()),
                Err(message)=>("failed".to_owned(),message),
            }
        };
        query("UPDATE albert_control_applications SET state=?,message=? WHERE id=?")
            .bind(state)
            .bind(message)
            .bind(&id)
            .execute(pool)
            .await
            .map_err(|_| ApiError::internal())?;
        self.application(permit, &id).await
    }
}
