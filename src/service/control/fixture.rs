use std::sync::{atomic::AtomicBool, Arc};

use octo_history::SqliteHistory;
use tempfile::{tempdir, TempDir};
use tokio::sync::Mutex;

use super::ControlService;
use crate::{
    api::{
        models::{CredentialUpdate, LoginRequest},
        principal::Principal,
    },
    config::control::ControlSettings,
    service::{
        auth::{AuthService, AuthSettings},
        session::SessionService,
    },
    sessions::settings::SessionSettings,
    web::runtime::AgentRuntime,
};

pub(super) async fn fixture() -> (TempDir, ControlService, Principal) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("albert.toml");
    let config = ControlSettings::from_path(&path).unwrap().base;
    let sessions = Arc::new(
        SessionService::from_history(
            Arc::new(
                SqliteHistory::open_retained(dir.path().join("history.db"))
                    .await
                    .unwrap(),
            ),
            SessionSettings::default(),
        )
        .await
        .unwrap(),
    );
    let auth = AuthService::open(
        sessions.database().pool().clone(),
        dir.path().join("key"),
        AuthSettings::default(),
    )
    .await
    .unwrap();
    let login = auth
        .login(LoginRequest {
            username: "owner".into(),
            password: "owner".into(),
        })
        .await
        .unwrap();
    let caller = auth.authenticate(&login.token).await.unwrap();
    let (_, login) = auth
        .rotate_credentials(
            &caller,
            CredentialUpdate {
                current_password: "owner".into(),
                new_password: "control test password".into(),
                new_username: None,
            },
        )
        .await
        .unwrap();
    let caller = auth.authenticate(&login.token).await.unwrap();
    let authority = config.authorization.build();
    let runtime = AgentRuntime::new(
        path,
        config,
        sessions,
        authority,
        Arc::new(AtomicBool::new(false)),
    );
    (
        dir,
        ControlService::new(runtime, Arc::new(Mutex::new(()))),
        caller,
    )
}
