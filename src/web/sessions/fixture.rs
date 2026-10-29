//! Shared real SQL/auth fixture for SessionHost boundary tests.
use std::sync::{atomic::AtomicBool, Arc};

use octo_history::SqliteHistory;
use tempfile::{tempdir, TempDir};

use super::SessionHost;
use crate::{
    api::{
        models::{CredentialUpdate, LoginRequest},
        principal::Principal,
    },
    config::Config,
    service::{
        auth::{AuthService, AuthSettings},
        session::SessionService,
    },
    sessions::settings::SessionSettings,
    web::runtime::AgentRuntime,
};

pub(super) async fn fixture() -> (TempDir, Arc<SessionHost>, Principal) {
    let dir = tempdir().unwrap();
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
        sessions.history().pool().clone(),
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
    let original = auth.authenticate(&login.token).await.unwrap();
    let (_, login) = auth
        .rotate_credentials(
            &original,
            CredentialUpdate {
                current_password: "owner".into(),
                new_password: "web-host-test-password".into(),
                new_username: None,
            },
        )
        .await
        .unwrap();
    let caller = auth.authenticate(&login.token).await.unwrap();
    auth.bind_profile(
        caller.account_id(),
        "private-web-owner",
        caller.account().revision,
    )
    .await
    .unwrap();
    let caller = auth.authenticate(&login.token).await.unwrap();
    let path = dir.path().join("albert.toml");
    let config = Config::parse_text("", &path).unwrap();
    let authority = config.authorization.build();
    let runtime = AgentRuntime::new(
        path,
        config,
        sessions,
        authority,
        Arc::new(AtomicBool::new(false)),
    );
    (dir, SessionHost::new(runtime, auth), caller)
}
