use crate::api::{
    errors::ApiError, errors::ApiResult, extractors::principal as authenticated_principal,
    extractors::COOKIE_NAME, host::HostOperation, host::ProfileOperation, models::AccountRole,
    models::Capabilities, models::ClientContext, models::SetupPhase, models::SetupView,
    state::ApiState,
};
use axum::{
    http::header::SET_COOKIE, http::HeaderMap, http::StatusCode, response::IntoResponse,
    response::Response, Json,
};

pub(super) async fn context(state: &ApiState, headers: &HeaderMap) -> ApiResult<Response> {
    let mut caller = match authenticated_principal(state, headers).await {
        Ok(caller) => Some(caller),
        Err(error) if error.status == StatusCode::UNAUTHORIZED => None,
        Err(error) => return Err(error),
    };
    if let Some(principal) = &caller {
        if !principal.must_rotate() && principal.canonical_user_id().is_none() {
            // Root owns the automatic configured-TG-owner/fresh-profile policy.
            state
                .host
                .operation(principal, HostOperation::Profile(ProfileOperation::Get))
                .await?;
            caller = Some(authenticated_principal(state, headers).await?);
        }
    }
    let runtime = state.host.status().await?;
    let account = caller.as_ref().map(|p| p.account().clone());
    let rotated = caller.as_ref().is_some_and(|p| !p.must_rotate());
    let owner = rotated
        && caller
            .as_ref()
            .is_some_and(|p| p.role() == AccountRole::Owner);
    let identity_required = rotated
        && caller
            .as_ref()
            .is_some_and(|p| p.canonical_user_id().is_none());
    let phase = if !rotated {
        SetupPhase::Credentials
    } else if identity_required {
        SetupPhase::Profile
    } else if runtime.error.is_some() {
        SetupPhase::RuntimeError
    } else if !runtime.model_ready {
        SetupPhase::Model
    } else {
        SetupPhase::Ready
    };
    Ok(Json(ClientContext {
        account,
        caps: Capabilities {
            owner,
            chat: rotated && !identity_required,
            manage_users: owner,
            manage_models: owner,
            manage_skills: owner,
            manage_config: owner,
        },
        setup: SetupView {
            phase,
            model_ready: runtime.model_ready,
            identity_required,
        },
        runtime_epoch: runtime.runtime_epoch,
        csrf: caller.as_ref().map(|p| p.csrf().to_owned()),
        current_credential_id: caller.as_ref().map(|p| p.credential_id().to_owned()),
    })
    .into_response())
}

pub(super) fn set_cookie(
    state: &ApiState,
    response: &mut Response,
    token: &str,
    ttl: u64,
    request_secure: bool,
) -> ApiResult<()> {
    let secure = request_secure
        || state.auth.settings().secure_cookie
        || state.auth.settings().public_origin.starts_with("https://");
    let cookie = format!(
        "{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={ttl}{}",
        if secure { "; Secure" } else { "" }
    );
    response.headers_mut().append(
        SET_COOKIE,
        cookie.parse().map_err(|_| ApiError::internal())?,
    );
    Ok(())
}
