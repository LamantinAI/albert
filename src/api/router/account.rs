use std::collections::HashMap;

use axum::{
    response::{IntoResponse, Response},
    Json,
};

use super::{id, parse, set_cookie, RouteSpec};
use crate::api::{
    errors::ApiResult,
    models::{CredentialUpdate, EmptyObject, UserCreate, UserList, UserUpdate},
    principal::Principal,
    state::ApiState,
};

pub(super) async fn account_request(
    state: &ApiState,
    caller: &Principal,
    spec: RouteSpec,
    params: &HashMap<String, String>,
    body: &[u8],
) -> ApiResult<Option<Response>> {
    Ok(Some(match (spec.method, spec.path) {
        ("DELETE", "/auth/credentials/{id}") => {
            let target = id(params, "id")?;
            state.auth.revoke_credential(caller, &target).await?;
            let mut response = Json(EmptyObject {}).into_response();
            if target == caller.credential_id() {
                set_cookie(state, &mut response, "", 0)?;
            }
            response
        }
        ("PUT", "/account/credentials") => {
            let input: CredentialUpdate = parse(body)?;
            let (account, login) = state.auth.rotate_credentials(caller, input).await?;
            let mut response = Json(account).into_response();
            set_cookie(
                state,
                &mut response,
                &login.token,
                state.auth.settings().credential_ttl_seconds,
            )?;
            response
        }
        ("GET", "/users") => Json(UserList {
            items: state.auth.list_users(&caller.owner_permit()?).await?,
        })
        .into_response(),
        ("POST", "/users") => {
            let input: UserCreate = parse(body)?;
            Json(
                state
                    .auth
                    .create_user(&caller.owner_permit()?, input)
                    .await?,
            )
            .into_response()
        }
        ("PATCH", "/users/{id}") => {
            let input: UserUpdate = parse(body)?;
            Json(
                state
                    .auth
                    .update_user(&caller.owner_permit()?, &id(params, "id")?, input)
                    .await?,
            )
            .into_response()
        }
        _ => return Ok(None),
    }))
}
