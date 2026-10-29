use crate::api::{errors::ApiResult, host::HostOperation, principal::Principal};

pub(super) fn host_access(caller: &Principal, operation: &HostOperation) -> ApiResult<()> {
    if matches!(
        operation,
        HostOperation::Model(_)
            | HostOperation::Skill(_)
            | HostOperation::Config(_)
            | HostOperation::Approval(_)
    ) {
        caller.owner_permit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use axum::http::{Method, StatusCode};
    use serde_json::json;

    use crate::api::{
        models::{AccountRole, CredentialUpdate, LoginRequest, UserCreate},
        testing::{fixture, request, ORIGIN},
    };

    #[tokio::test]
    async fn ordinary_rotated_user_cannot_manage_global_resources_or_another_credential() {
        let fixture = fixture().await;
        let login = fixture
            .auth
            .login(LoginRequest {
                username: "owner".into(),
                password: "owner".into(),
            })
            .await
            .unwrap();
        let initial = fixture.auth.authenticate(&login.token).await.unwrap();
        let (_, owner_login) = fixture
            .auth
            .rotate_credentials(
                &initial,
                CredentialUpdate {
                    current_password: "owner".into(),
                    new_password: "rotated-owner-password".into(),
                    new_username: None,
                },
            )
            .await
            .unwrap();
        let owner = fixture.auth.authenticate(&owner_login.token).await.unwrap();
        fixture
            .auth
            .create_user(
                &owner.owner_permit().unwrap(),
                UserCreate {
                    username: "regular".into(),
                    password: "initial-user-password".into(),
                    role: AccountRole::User,
                },
            )
            .await
            .unwrap();
        let user_login = fixture
            .auth
            .login(LoginRequest {
                username: "regular".into(),
                password: "initial-user-password".into(),
            })
            .await
            .unwrap();
        let initial_user = fixture.auth.authenticate(&user_login.token).await.unwrap();
        let (_, user_login) = fixture
            .auth
            .rotate_credentials(
                &initial_user,
                CredentialUpdate {
                    current_password: "initial-user-password".into(),
                    new_password: "rotated-user-password".into(),
                    new_username: None,
                },
            )
            .await
            .unwrap();
        let user = fixture.auth.authenticate(&user_login.token).await.unwrap();
        let cookie = format!("albert_auth={}", user_login.token);
        for path in [
            "/api/v1/users",
            "/api/v1/model-pool",
            "/api/v1/secrets",
            "/api/v1/skills",
        ] {
            assert_eq!(
                request(
                    &fixture.router,
                    Method::GET,
                    path,
                    Some(&cookie),
                    None,
                    None,
                    json!({})
                )
                .await
                .0,
                StatusCode::FORBIDDEN
            );
        }
        assert_eq!(
            request(
                &fixture.router,
                Method::PUT,
                "/api/v1/model-pool/preference",
                Some(&cookie),
                Some(user.csrf()),
                Some(ORIGIN),
                json!({"model_id":"attempted-owner-write"})
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            request(
                &fixture.router,
                Method::POST,
                "/api/v1/sessions",
                Some(&cookie),
                Some(user.csrf()),
                Some(ORIGIN),
                json!({"title":"forged","user_id":owner.account_id()})
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(fixture.host.calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            request(
                &fixture.router,
                Method::DELETE,
                &format!("/api/v1/auth/credentials/{}", owner.credential_id()),
                Some(&cookie),
                Some(user.csrf()),
                Some(ORIGIN),
                json!({})
            )
            .await
            .0,
            StatusCode::OK
        );
        assert!(fixture.auth.authenticate(&owner_login.token).await.is_ok());
        assert_eq!(
            fixture.host.calls.load(Ordering::SeqCst),
            0,
            "browser logout never ends a conversation"
        );
    }
}
