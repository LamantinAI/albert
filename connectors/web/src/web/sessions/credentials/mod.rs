use albert_core::{
    credentials::CredentialError, selfconfig::SelfConfig, sessions::actor::ActorToolPolicy,
};
use axum::http::StatusCode;

use super::{session_error, SessionHost};
use crate::{
    api::principal::Principal,
    api::{
        errors::{ApiError, ApiResult},
        host::{CredentialOperation, HostResponse},
        models::{CredentialRequestList, CredentialRequestView},
    },
};

fn problem(error: CredentialError) -> ApiError {
    let status = match error {
        CredentialError::NotFound => StatusCode::NOT_FOUND,
        CredentialError::Denied => StatusCode::FORBIDDEN,
        CredentialError::Name | CredentialError::Value => StatusCode::BAD_REQUEST,
        _ => StatusCode::CONFLICT,
    };
    ApiError::new(status, "credential_request", error.to_string())
}
impl SessionHost {
    pub async fn credential_operation(
        &self,
        caller: &Principal,
        operation: CredentialOperation,
    ) -> ApiResult<HostResponse> {
        let caller = self.auth.validate_principal(caller).await?;
        caller.owner_permit()?;
        let actor = self.actor(&caller).await?;
        if actor.tools != ActorToolPolicy::ExistingHost {
            return Err(ApiError::forbidden());
        }
        let guard = self.runtime.sessions.credentials();
        let user = actor.user_id.as_str();
        match operation {
            CredentialOperation::List { session_id } => {
                if let Some(id) = &session_id {
                    self.resource(&actor, id).await?;
                }
                Ok(HostResponse::CredentialRequests(CredentialRequestList {
                    existing_names: SelfConfig::new(self.runtime.base.deploy_dir.clone())
                        .panel_secret_names(),
                    items: guard
                        .list(&actor.account_id, user, session_id.as_deref())
                        .into_iter()
                        .map(CredentialRequestView::from)
                        .collect(),
                }))
            }
            CredentialOperation::Create(input) => {
                let context = if let Some(id) = &input.session_id {
                    let context = self.resource(&actor, id).await?;
                    let grant = self
                        .runtime
                        .sessions
                        .accounts()
                        .account(&actor.account_id)
                        .await
                        .map_err(|_| ApiError::forbidden())?;
                    Some(context.with_account(grant))
                } else {
                    None
                };
                let view = guard
                    .create(
                        &actor.account_id,
                        user,
                        &input.client_request_id,
                        &input.name,
                        context,
                        None,
                    )
                    .map_err(problem)?;
                Ok(HostResponse::CredentialRequest(view.into()))
            }
            CredentialOperation::Get { id } => Ok(HostResponse::CredentialRequest(
                guard
                    .get(&actor.account_id, user, &id)
                    .map_err(problem)?
                    .into(),
            )),
            CredentialOperation::Cancel { id } => Ok(HostResponse::CredentialRequest(
                guard
                    .cancel(&actor.account_id, user, &id)
                    .map_err(problem)?
                    .into(),
            )),
            CredentialOperation::Submit { id, input } => {
                let view = guard.get(&actor.account_id, user, &id).map_err(problem)?;
                // The lifecycle reader fences End/Save while the owner-approved write commits.
                let _lease = self
                    .runtime
                    .sessions
                    .journal_router()
                    .lifecycle_gate()
                    .read_owned()
                    .await;
                if let Some(session) = &view.session_id {
                    let context = self.resource(&actor, session).await?;
                    self.runtime
                        .sessions
                        .validate_context(&context)
                        .await
                        .map_err(session_error)?;
                }
                self.auth
                    .validate_principal(&caller)
                    .await?
                    .owner_permit()?;
                if input.value.is_empty()
                    || input.value.len() > guard.settings().max_value_bytes
                    || input.value.contains(['\n', '\r', '\0'])
                {
                    return Err(problem(CredentialError::Value));
                }
                let store = SelfConfig::new(self.runtime.base.deploy_dir.clone());
                let result = guard
                    .save(&actor.account_id, user, &id, &input.name, || {
                        store
                            .panel_upsert_secret(&input.name, &input.value)
                            .map_err(|_| ())
                    })
                    .map_err(problem)?;
                drop(_lease);
                self.runtime.sessions.record_manual_credentials().await;
                let result = guard.get(&actor.account_id, user, &id).unwrap_or(result);
                Ok(HostResponse::CredentialRequest(result.into()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs::read_to_string;

    use axum::http::{Method, StatusCode};
    use futures::StreamExt;
    use octo_history::{SessionId, SessionKind};
    use serde_json::{json, to_value};
    use sqlx::query;
    use tokio::time::{timeout, Duration};

    use crate::{
        api::{
            host::{CredentialOperation, SessionEventRequest, SessionOperation},
            models::{CredentialCreate, CredentialSubmission, LoginRequest, PageQuery},
            router as api_router,
            state::ApiState,
            testing::{request as http, ORIGIN},
        },
        web::{sessions::fixture::fixture, Panel},
    };
    use albert_core::selfconfig::SelfConfig;

    #[tokio::test]
    async fn scoped_save_exposes_only_actual_name_and_duplicate_does_not_write_again() {
        let (dir, host, caller) = fixture().await;
        let actor = host.actor(&caller).await.unwrap();
        let session = host
            .runtime
            .sessions
            .web_create(&actor, "Credential scope", false)
            .await
            .unwrap();
        let id = SessionId::new().to_string();
        let created = host
            .credential_operation(
                &caller,
                CredentialOperation::Create(CredentialCreate {
                    client_request_id: id.clone(),
                    session_id: Some(session.id.to_string()),
                    name: "SUGGESTED".into(),
                }),
            )
            .await
            .unwrap();
        assert!(!to_value(created).unwrap().to_string().contains("value"));
        let sentinel = "sensitive-fixture-value-49";
        let saved = host
            .credential_operation(
                &caller,
                CredentialOperation::Submit {
                    id: id.clone(),
                    input: CredentialSubmission {
                        name: "ACTUAL".into(),
                        value: sentinel.into(),
                    },
                },
            )
            .await
            .unwrap();
        let payload = to_value(saved).unwrap();
        assert_eq!(payload["name"], "ACTUAL");
        assert_eq!(payload["state"], "saved");
        assert!(!payload.to_string().contains(sentinel));
        let duplicate = host
            .credential_operation(
                &caller,
                CredentialOperation::Submit {
                    id: id.clone(),
                    input: CredentialSubmission {
                        name: "OTHER".into(),
                        value: "replacement".into(),
                    },
                },
            )
            .await
            .unwrap();
        assert_eq!(to_value(duplicate).unwrap()["name"], "ACTUAL");
        let names = host
            .credential_operation(
                &caller,
                CredentialOperation::List {
                    session_id: Some(session.id.to_string()),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            to_value(names).unwrap()["existing_names"],
            json!(["ACTUAL"])
        );
        let file = read_to_string(dir.path().join(".env")).unwrap();
        assert!(file.contains(sentinel));
        assert!(!file.contains("replacement"));
        assert!(!file.contains("SUGGESTED="));
        let context = host.resource(&actor, session.id.as_str()).await.unwrap();
        let history = host
            .runtime
            .sessions
            .journal_router()
            .strict_load(&context.root_history_key())
            .await
            .unwrap();
        assert_eq!(history.len(), 1);
        assert!(history[0].content.contains("ACTUAL"));
        assert!(!history[0].content.contains(sentinel));
    }
    #[tokio::test]
    async fn manual_writes_from_an_unsent_or_temporary_chat_do_not_create_or_promote_a_chat() {
        let (_dir, host, caller) = fixture().await;
        let id = SessionId::new().to_string();
        host.credential_operation(
            &caller,
            CredentialOperation::Create(CredentialCreate {
                client_request_id: id.clone(),
                session_id: None,
                name: "".into(),
            }),
        )
        .await
        .unwrap();
        host.credential_operation(
            &caller,
            CredentialOperation::Submit {
                id,
                input: CredentialSubmission {
                    name: "MANUAL".into(),
                    value: "fixture".into(),
                },
            },
        )
        .await
        .unwrap();
        let listed = host
            .operation(&caller, SessionOperation::List(PageQuery::default()))
            .await
            .unwrap();
        assert!(to_value(listed).unwrap()["items"]
            .as_array()
            .unwrap()
            .is_empty());
        let actor = host.actor(&caller).await.unwrap();
        let session = host
            .runtime
            .sessions
            .web_create(&actor, "Temporary credential", true)
            .await
            .unwrap();
        let id = SessionId::new().to_string();
        host.credential_operation(
            &caller,
            CredentialOperation::Create(CredentialCreate {
                client_request_id: id.clone(),
                session_id: Some(session.id.to_string()),
                name: "".into(),
            }),
        )
        .await
        .unwrap();
        host.credential_operation(
            &caller,
            CredentialOperation::Submit {
                id,
                input: CredentialSubmission {
                    name: "TEMP_MANUAL".into(),
                    value: "fixture".into(),
                },
            },
        )
        .await
        .unwrap();
        assert_eq!(
            host.resource(&actor, session.id.as_str())
                .await
                .unwrap()
                .session()
                .kind,
            SessionKind::Temporary
        );
    }
    #[tokio::test]
    async fn credentials_have_independent_sse_events_after_snapshot_without_a_live_turn() {
        let (_dir, host, caller) = fixture().await;
        let actor = host.actor(&caller).await.unwrap();
        let session = host
            .runtime
            .sessions
            .web_create(&actor, "Credential events", false)
            .await
            .unwrap();
        let mut stream = host
            .events(
                caller.clone(),
                SessionEventRequest {
                    session_id: session.id.to_string(),
                    after: None,
                    runtime_epoch: None,
                },
            )
            .await
            .unwrap();
        timeout(Duration::from_secs(1), stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let id = SessionId::new().to_string();
        host.credential_operation(
            &caller,
            CredentialOperation::Create(CredentialCreate {
                client_request_id: id.clone(),
                session_id: Some(session.id.to_string()),
                name: "API_TOKEN".into(),
            }),
        )
        .await
        .unwrap();
        let event = timeout(Duration::from_secs(1), stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(event.kind, "credentials");
        assert_eq!(event.data, json!({"request_id":id}));
        assert_eq!(event.attempt_id, None);
    }
    #[tokio::test]
    async fn authenticated_route_never_echoes_malformed_values_and_checks_csrf_and_fresh_role() {
        let (_dir, host, _caller) = fixture().await;
        let panel = Panel::new(host.runtime.clone(), host.auth.clone());
        let router = api_router(ApiState::new(host.auth.clone(), panel));
        let login = host
            .auth
            .login(LoginRequest {
                username: "owner".into(),
                password: "web-host-test-password".into(),
            })
            .await
            .unwrap();
        let principal = host.auth.authenticate(&login.token).await.unwrap();
        let cookie = format!("albert_auth={}", login.token);
        let id = SessionId::new().to_string();
        let (status, _, _) = http(
            &router,
            Method::POST,
            "/api/v1/credential-requests",
            Some(&cookie),
            None,
            Some(ORIGIN),
            json!({"client_request_id":id,"name":""}),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _, _) = http(
            &router,
            Method::POST,
            "/api/v1/credential-requests",
            Some(&cookie),
            Some(principal.csrf()),
            Some(ORIGIN),
            json!({"client_request_id":id,"name":""}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let sentinel = "sensitive-fixture-value-49";
        let path = format!("/api/v1/credential-requests/{id}/submissions");
        let (status, _, error) = http(
            &router,
            Method::POST,
            &path,
            Some(&cookie),
            Some(principal.csrf()),
            Some(ORIGIN),
            json!({"name":"TOKEN","value":{"secret":sentinel}}),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(!error.to_string().contains(sentinel));
        query("UPDATE albert_login_accounts SET role='user' WHERE id=?")
            .bind(principal.account_id())
            .execute(host.runtime.sessions.database().pool())
            .await
            .unwrap();
        let (status, _, error) = http(
            &router,
            Method::POST,
            &path,
            Some(&cookie),
            Some(principal.csrf()),
            Some(ORIGIN),
            json!({"name":"TOKEN","value":sentinel}),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(!error.to_string().contains(sentinel));
        let login = host
            .auth
            .login(LoginRequest {
                username: "owner".into(),
                password: "web-host-test-password".into(),
            })
            .await
            .unwrap();
        let user = host.auth.authenticate(&login.token).await.unwrap();
        let cookie = format!("albert_auth={}", login.token);
        let (status, _, error) = http(
            &router,
            Method::POST,
            &path,
            Some(&cookie),
            Some(user.csrf()),
            Some(ORIGIN),
            json!({"name":"TOKEN","value":sentinel}),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(!error.to_string().contains(sentinel));
        assert!(!host.runtime.base.deploy_dir.join(".env").exists());
    }
    #[tokio::test]
    async fn saved_secret_survives_a_failed_metadata_commit_without_being_written_again() {
        let (dir, host, caller) = fixture().await;
        let actor = host.actor(&caller).await.unwrap();
        let session = host
            .runtime
            .sessions
            .web_create(&actor, "Receipt failure", false)
            .await
            .unwrap();
        let id = SessionId::new().to_string();
        host.credential_operation(
            &caller,
            CredentialOperation::Create(CredentialCreate {
                client_request_id: id.clone(),
                session_id: Some(session.id.to_string()),
                name: "".into(),
            }),
        )
        .await
        .unwrap();
        let pool = host.runtime.sessions.database().pool();
        query("CREATE TRIGGER reject_credential_notice BEFORE INSERT ON journal_appends WHEN NEW.namespace='albert/credential-manual' BEGIN SELECT RAISE(ABORT,'metadata receipt failure'); END").execute(pool).await.unwrap();
        let result = host
            .credential_operation(
                &caller,
                CredentialOperation::Submit {
                    id: id.clone(),
                    input: CredentialSubmission {
                        name: "STORED_TOKEN".into(),
                        value: "first-fixture".into(),
                    },
                },
            )
            .await
            .unwrap();
        let result = to_value(result).unwrap();
        assert_eq!(result["state"], "saved");
        assert_eq!(result["notification"], "pending");
        SelfConfig::new(dir.path().into())
            .panel_set_secret("STORED_TOKEN", "newer-fixture")
            .unwrap();
        query("DROP TRIGGER reject_credential_notice")
            .execute(pool)
            .await
            .unwrap();
        host.runtime.sessions.record_manual_credentials().await;
        host.runtime.sessions.record_manual_credentials().await;
        let receipt = host
            .credential_operation(&caller, CredentialOperation::Get { id })
            .await
            .unwrap();
        assert_eq!(to_value(receipt).unwrap()["notification"], "recorded");
        assert!(read_to_string(dir.path().join(".env"))
            .unwrap()
            .contains("newer-fixture"));
        let context = host.resource(&actor, session.id.as_str()).await.unwrap();
        let history = host
            .runtime
            .sessions
            .journal_router()
            .strict_load(&context.root_history_key())
            .await
            .unwrap();
        assert_eq!(history.len(), 1);
        assert!(!history[0].content.contains("first-fixture"));
        assert!(!history[0].content.contains("newer-fixture"));
    }
}
