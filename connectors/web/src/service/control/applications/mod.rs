use std::{sync::Arc, time::Duration};

use albert_core::runtime::AgentRuntime;
use rand::random;
use sqlx::{query, query_as, query_scalar, SqlitePool};
use tokio::{spawn, time::sleep};
use tracing::warn;

use super::ControlService;
use crate::{
    api::errors::{ApiError, ApiResult},
    api::models::{ApplyRequest, ApplyView},
    api::principal::OwnerPermit,
    service::panel_models::files::hash,
    settings::validate_control_file,
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
    async fn control_owner(&self, permit: &OwnerPermit) -> ApiResult<()> {
        let live:i64=query_scalar("SELECT EXISTS(SELECT 1 FROM albert_login_accounts WHERE id=? AND role='owner' AND disabled=0 AND must_change_password=0)")
            .bind(permit.principal().account_id()).fetch_one(self.runtime.sessions.database().pool()).await.map_err(|_|ApiError::internal())?;
        if live == 1 {
            Ok(())
        } else {
            Err(ApiError::forbidden())
        }
    }
    pub(super) async fn application(&self, permit: &OwnerPermit, id: &str) -> ApiResult<ApplyView> {
        self.control_owner(permit).await?;
        let quiet = self.gate.try_lock().ok();
        self.application_record(permit, id, quiet.is_some()).await
    }
    async fn application_record(
        &self,
        permit: &OwnerPermit,
        id: &str,
        quiescent: bool,
    ) -> ApiResult<ApplyView> {
        let pool = self.runtime.sessions.database().pool();
        let row:Option<Stored>=query_as("SELECT id,request_hash,revision,state,runtime_epoch,message,document_id FROM albert_control_applications WHERE id=? AND account_id=?")
            .bind(id).bind(permit.principal().account_id()).fetch_optional(pool).await.map_err(|_|ApiError::internal())?;
        let Some((id, _, revision, mut state, mut epoch, mut message, document_id)) = row else {
            return Err(ApiError::not_found());
        };
        let runtime = self.runtime.status();
        let previous = (state.clone(), epoch.clone());
        if state == "pending" && (quiescent || epoch != runtime.runtime_epoch) {
            state = "interrupted".into();
            message="The operation ended before its result was recorded. Its effects may have occurred. Check the current runtime before starting another command.".into();
        } else if state == "restarting"
            && epoch != runtime.runtime_epoch
            && runtime.phase != "starting"
        {
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
        }
        if state != previous.0 {
            let updated=query("UPDATE albert_control_applications SET state=?,runtime_epoch=?,message=? WHERE id=? AND state=? AND runtime_epoch=?")
                .bind(&state).bind(&epoch).bind(&message).bind(&id).bind(&previous.0).bind(&previous.1).execute(pool).await.map_err(|_|ApiError::internal())?;
            if updated.rows_affected() != 1 {
                return Err(ApiError::conflict(
                    "Application state changed. Reload its receipt.",
                ));
            }
        }
        Ok(ApplyView {
            id,
            revision,
            state,
            runtime_epoch: epoch,
            message,
        })
    }
    async fn application_state(&self, id: &str, state: &str, message: &str) -> ApiResult<()> {
        let updated=query("UPDATE albert_control_applications SET state=?,message=? WHERE id=? AND state='pending'")
            .bind(state).bind(message).bind(id).execute(self.runtime.sessions.database().pool()).await.map_err(|_|ApiError::internal())?;
        if updated.rows_affected() != 1 {
            return Err(ApiError::conflict(
                "Application receipt changed before its result was recorded.",
            ));
        }
        Ok(())
    }
    pub(super) async fn apply(
        &self,
        permit: &OwnerPermit,
        input: ApplyRequest,
    ) -> ApiResult<ApplyView> {
        if !["runtime", "process", "models"].contains(&input.target.as_str())
            || input.client_request_id.is_empty()
            || input.client_request_id.len() > 128
        {
            return Err(ApiError::invalid(
                "Choose runtime, models or process and provide a client request ID.",
            ));
        }
        let request_hash = hash(&[
            input.target.as_bytes(),
            input.document_id.as_deref().unwrap_or("").as_bytes(),
            input.revision.as_deref().unwrap_or("").as_bytes(),
        ]);
        let _gate = self.gate.lock().await;
        self.control_owner(permit).await?;
        let pool = self.runtime.sessions.database().pool();
        let previous:Option<(String,String)>=query_as("SELECT id,request_hash FROM albert_control_applications WHERE account_id=? AND client_request_id=?")
            .bind(permit.principal().account_id()).bind(&input.client_request_id).fetch_optional(pool).await.map_err(|_|ApiError::internal())?;
        if let Some((id, expected)) = previous {
            if expected != request_hash {
                return Err(ApiError::conflict(
                    "This application request ID was used for different settings.",
                ));
            }
            return self.application_record(permit, &id, true).await;
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
        let (state, message) = if input.target == "process" {
            if validate_control_file(&self.runtime.path).is_err() {
                (
                    "failed".to_owned(),
                    "Invalid control-plane settings. Correct them before restarting.".to_owned(),
                )
            } else if self.runtime.context().is_none() {
                (
                    "failed".to_owned(),
                    "Runtime is not started. Reconnect before restarting.".to_owned(),
                )
            } else {
                // A durable restarting receipt must exist before shutdown can be scheduled.
                self.application_state(
                    &id,
                    "restarting",
                    "Restart requested. The panel will reconnect automatically.",
                )
                .await?;
                spawn(restart_after_receipt(
                    self.runtime.clone(),
                    pool.clone(),
                    id.clone(),
                    epoch,
                ));
                return self.application_record(permit, &id, false).await;
            }
        } else if input.target == "models" {
            match self.runtime.current().ok_or_else(||"Connect a model before reloading the pool.".to_owned()).and_then(|agent|agent.panel_reload_models()) {
                Ok(selected)=>("applied".to_owned(),format!("Model pool reloaded. Preferred model: {selected}. Running turns keep their snapshot.")),
                Err(message)=>("failed".to_owned(),message),
            }
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
        self.application_state(&id, &state, &message).await?;
        self.application_record(permit, &id, false).await
    }
}

async fn restart_after_receipt(
    runtime: Arc<AgentRuntime>,
    pool: SqlitePool,
    id: String,
    epoch: String,
) {
    sleep(Duration::from_millis(300)).await;
    if let Err(message) = runtime.request_restart() {
        if query("UPDATE albert_control_applications SET state='failed',message=? WHERE id=? AND state='restarting' AND runtime_epoch=?")
            .bind(message).bind(id).bind(epoch).execute(&pool).await.is_err() {
            warn!("Could not record the failed runtime restart request");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs::{create_dir_all, write},
        sync::{atomic::Ordering, Arc},
        time::Duration,
    };

    use albert_core::runtime::RuntimeCogitator;
    use axum::http::StatusCode;
    use octo_core::{
        Cogitator, CogitatorContext, EventBus, Filter, InProcessBus, SubscribeOptions,
    };
    use sqlx::{query, query_scalar};
    use tokio::{
        join, spawn,
        task::{yield_now, JoinHandle},
        time::{sleep, timeout},
    };

    use super::{restart_after_receipt, ControlService};
    use crate::{api::models::ApplyRequest, service::control::fixture::fixture};

    async fn start(service: &ControlService) -> JoinHandle<()> {
        let bus = Arc::new(InProcessBus::new(32));
        let events = bus
            .subscribe(Filter::all(), SubscribeOptions::default())
            .await
            .unwrap();
        let context = CogitatorContext::new(Default::default(), bus, Vec::new());
        let proxy = Arc::new(RuntimeCogitator(service.runtime.clone()));
        let task = spawn(async move {
            proxy.run(context, events).await.unwrap();
        });
        timeout(Duration::from_secs(2), async {
            while service.runtime.context().is_none() {
                yield_now().await;
            }
        })
        .await
        .unwrap();
        task
    }
    fn request(service: &ControlService, id: &str, target: &str) -> ApplyRequest {
        ApplyRequest {
            target: target.into(),
            client_request_id: id.into(),
            document_id: None,
            revision: Some(service.revision().unwrap()),
        }
    }
    #[tokio::test]
    async fn restart_is_committed_before_effect_concurrent_retries_and_new_epochs_do_not_repeat_it()
    {
        let (_dir, service, caller) = fixture().await;
        let task = start(&service).await;
        let permit = caller.owner_permit().unwrap();
        let (first, second) = join!(
            service.apply(&permit, request(&service, "same-command", "process")),
            service.apply(&permit, request(&service, "same-command", "process"))
        );
        let first = first.unwrap();
        let second = second.unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(first.state, "restarting");
        assert!(!service.runtime.restart_requested.load(Ordering::Acquire));
        let state: String =
            query_scalar("SELECT state FROM albert_control_applications WHERE id=?")
                .bind(&first.id)
                .fetch_one(service.runtime.sessions.database().pool())
                .await
                .unwrap();
        assert_eq!(state, "restarting");
        timeout(Duration::from_secs(2), async {
            while !service.runtime.restart_requested.load(Ordering::Acquire) {
                sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
        service
            .runtime
            .restart_requested
            .store(false, Ordering::Release);
        query("UPDATE albert_control_applications SET runtime_epoch='previous-runtime' WHERE id=?")
            .bind(&first.id)
            .execute(service.runtime.sessions.database().pool())
            .await
            .unwrap();
        let replay = service
            .apply(&permit, request(&service, "same-command", "process"))
            .await
            .unwrap();
        assert_eq!(replay.state, "applied");
        sleep(Duration::from_millis(350)).await;
        assert!(!service.runtime.restart_requested.load(Ordering::Acquire));
        assert!(service
            .apply(&permit, request(&service, "same-command", "models"))
            .await
            .is_err());
        let rows: i64 = query_scalar("SELECT count(*) FROM albert_control_applications")
            .fetch_one(service.runtime.sessions.database().pool())
            .await
            .unwrap();
        assert_eq!(rows, 1);
    }
    #[tokio::test]
    async fn receipt_update_failure_prevents_scheduling_and_replay_does_not_execute_interrupted_work(
    ) {
        let (_dir, service, caller) = fixture().await;
        let task = start(&service).await;
        let permit = caller.owner_permit().unwrap();
        let pool = service.runtime.sessions.database().pool();
        query("CREATE TRIGGER reject_restart_receipt BEFORE UPDATE OF state ON albert_control_applications WHEN NEW.state='restarting' BEGIN SELECT RAISE(ABORT,'receipt update failure'); END").execute(pool).await.unwrap();
        assert!(service
            .apply(&permit, request(&service, "no-receipt", "process"))
            .await
            .is_err());
        sleep(Duration::from_millis(350)).await;
        assert!(!service.runtime.restart_requested.load(Ordering::Acquire));
        query("DROP TRIGGER reject_restart_receipt")
            .execute(pool)
            .await
            .unwrap();
        let recovered = service
            .apply(&permit, request(&service, "no-receipt", "process"))
            .await
            .unwrap();
        assert_eq!(recovered.state, "interrupted");
        assert!(recovered.message.contains("effects may have occurred"));
        let context = service.runtime.context().unwrap();
        context.shutdown.cancel();
        timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
    }
    #[tokio::test]
    async fn no_acceptance_and_no_current_owner_means_no_effect() {
        let (_dir, service, caller) = fixture().await;
        let task = start(&service).await;
        let permit = caller.owner_permit().unwrap();
        let pool = service.runtime.sessions.database().pool();
        query("CREATE TRIGGER reject_control_insert BEFORE INSERT ON albert_control_applications BEGIN SELECT RAISE(ABORT,'receipt insert failure'); END").execute(pool).await.unwrap();
        assert!(service
            .apply(&permit, request(&service, "not-admitted", "process"))
            .await
            .is_err());
        query("DROP TRIGGER reject_control_insert")
            .execute(pool)
            .await
            .unwrap();
        query("UPDATE albert_login_accounts SET role='user' WHERE id=?")
            .bind(caller.account_id())
            .execute(pool)
            .await
            .unwrap();
        assert_eq!(
            service
                .apply(&permit, request(&service, "revoked-owner", "models"))
                .await
                .err()
                .unwrap()
                .status,
            StatusCode::FORBIDDEN
        );
        sleep(Duration::from_millis(350)).await;
        assert!(!service.runtime.restart_requested.load(Ordering::Acquire));
        let rows: i64 = query_scalar("SELECT count(*) FROM albert_control_applications")
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(rows, 0);
        service.runtime.context().unwrap().shutdown.cancel();
        timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
    }
    #[tokio::test]
    async fn failed_restart_request_is_recorded_and_pending_receipts_are_not_resubmitted() {
        let (_dir, service, caller) = fixture().await;
        let permit = caller.owner_permit().unwrap();
        let pool = service.runtime.sessions.database().pool();
        query("INSERT INTO albert_control_applications(id,account_id,client_request_id,request_hash,target,revision,state,message,runtime_epoch) VALUES ('failed-restart',?,'stable','hash','process','revision','restarting','Requested',?)").bind(caller.account_id()).bind(service.runtime.status().runtime_epoch).execute(pool).await.unwrap();
        restart_after_receipt(
            service.runtime.clone(),
            pool.clone(),
            "failed-restart".into(),
            service.runtime.status().runtime_epoch,
        )
        .await;
        let record = service
            .application(&permit, "failed-restart")
            .await
            .unwrap();
        assert_eq!(record.state, "failed");
        assert!(!service.runtime.restart_requested.load(Ordering::Acquire));
        query("UPDATE albert_control_applications SET state='pending' WHERE id='failed-restart'")
            .execute(pool)
            .await
            .unwrap();
        let record = service
            .application(&permit, "failed-restart")
            .await
            .unwrap();
        assert_eq!(record.state, "interrupted");
    }
    #[tokio::test]
    async fn pool_reload_keeps_running_snapshots_and_does_not_refresh_skills() {
        let (dir, service, caller) = fixture().await;
        let pool_file = dir.path().join("models.toml");
        write(
            dir.path().join(".env"),
            format!(
                "ALBERT_OPENAI_KEY=fixture-placeholder\nKAERU_VAULT_PATH={}\n",
                dir.path().join("memory").display()
            ),
        )
        .unwrap();
        write(
            &service.runtime.path,
            "model_pool='models.toml'\nreflection_secs=0\n",
        )
        .unwrap();
        let document = |id: &str| {
            format!("default='{id}'\n[[models]]\nid='{id}'\nmodel='fixture-model'\nprovider='api_key'\nvision=false\ntools=true\n")
        };
        write(&pool_file, document("first")).unwrap();
        let task = start(&service).await;
        service.runtime.activate().await.unwrap();
        let agent = service.runtime.current().unwrap();
        let snapshot = agent.panel_models();
        assert_eq!(snapshot.selected, "first");
        create_dir_all(dir.path().join("skills/later")).unwrap();
        write(
            dir.path().join("skills/later/SKILL.md"),
            "---\nname: Later skill\n---\nUnused new instructions",
        )
        .unwrap();
        write(&pool_file, document("second")).unwrap();
        let permit = caller.owner_permit().unwrap();
        let result = service
            .apply(&permit, request(&service, "reload-pool", "models"))
            .await
            .unwrap();
        assert_eq!(result.state, "applied");
        assert_eq!(agent.panel_models().selected, "second");
        assert_eq!(snapshot.selected, "first");
        assert!(agent.panel_composer_skills().is_empty());
        assert!(Arc::ptr_eq(&agent, &service.runtime.current().unwrap()));
        write(&pool_file, "invalid model configuration").unwrap();
        let result = service
            .apply(&permit, request(&service, "invalid-pool", "models"))
            .await
            .unwrap();
        assert_eq!(result.state, "failed");
        assert_eq!(agent.panel_models().selected, "second");
        service.runtime.context().unwrap().shutdown.cancel();
        timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
    }
}
