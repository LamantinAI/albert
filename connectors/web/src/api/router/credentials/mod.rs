//! credentials HTTP resources.
use axum::{extract::State, Json};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::{
    actor::OwnerActor, host::run, input::resource, input::ApiJson, input::ApiPath, input::ApiQuery,
};
use crate::api::models::{
    ApiErrorBody, CredentialCreate, CredentialQuery, CredentialRequestList, CredentialRequestView,
    CredentialSubmission, EmptyObject,
};
use crate::api::{
    errors::ApiResult, host::CredentialOperation, host::HostOperation, state::ApiState,
};

pub(super) fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(get_credential_requests))
        .routes(routes!(post_credential_requests))
        .routes(routes!(get_credential_requests_id))
        .routes(routes!(post_credential_requests_id_submissions))
        .routes(routes!(post_credential_requests_id_cancellations))
}

#[utoipa::path(get, operation_id = "credentials_get_credential_requests", path = "/credential-requests", summary = "Get credential requests", tag = "credentials",
    params(CredentialQuery),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = CredentialRequestList),
        (status = 400, description = "Invalid request", body = ApiErrorBody),
        (status = 401, description = "Sign-in required", body = ApiErrorBody),
        (status = 403, description = "Insufficient authority", body = ApiErrorBody),
        (status = 404, description = "Resource unavailable", body = ApiErrorBody),
        (status = 409, description = "Conflicting state", body = ApiErrorBody),
        (status = 412, description = "Revision changed", body = ApiErrorBody),
        (status = 413, description = "Body too large", body = ApiErrorBody),
        (status = 503, description = "Service unavailable", body = ApiErrorBody),
        (status = 500, description = "Internal failure", body = ApiErrorBody))
)]
async fn get_credential_requests(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiQuery(query): ApiQuery<CredentialQuery>,
) -> ApiResult<Json<CredentialRequestList>> {
    let operation = HostOperation::Credential(CredentialOperation::List {
        session_id: query.session_id,
    });
    run(&state, &caller, operation).await
}

#[utoipa::path(post, operation_id = "credentials_post_credential_requests", path = "/credential-requests", summary = "Create credential requests", tag = "credentials",
    request_body = CredentialCreate,
    params(("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = CredentialRequestView),
        (status = 400, description = "Invalid request", body = ApiErrorBody),
        (status = 401, description = "Sign-in required", body = ApiErrorBody),
        (status = 403, description = "Insufficient authority", body = ApiErrorBody),
        (status = 404, description = "Resource unavailable", body = ApiErrorBody),
        (status = 409, description = "Conflicting state", body = ApiErrorBody),
        (status = 412, description = "Revision changed", body = ApiErrorBody),
        (status = 413, description = "Body too large", body = ApiErrorBody),
        (status = 503, description = "Service unavailable", body = ApiErrorBody),
        (status = 500, description = "Internal failure", body = ApiErrorBody))
)]
async fn post_credential_requests(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiJson(input): ApiJson<CredentialCreate>,
) -> ApiResult<Json<CredentialRequestView>> {
    let operation = { HostOperation::Credential(CredentialOperation::Create(input)) };
    run(&state, &caller, operation).await
}

#[utoipa::path(get, operation_id = "credentials_get_credential_requests_id", path = "/credential-requests/{id}", summary = "Get credential requests", tag = "credentials",
    params(("id" = String, Path, description = "Resource identifier")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = CredentialRequestView),
        (status = 400, description = "Invalid request", body = ApiErrorBody),
        (status = 401, description = "Sign-in required", body = ApiErrorBody),
        (status = 403, description = "Insufficient authority", body = ApiErrorBody),
        (status = 404, description = "Resource unavailable", body = ApiErrorBody),
        (status = 409, description = "Conflicting state", body = ApiErrorBody),
        (status = 412, description = "Revision changed", body = ApiErrorBody),
        (status = 413, description = "Body too large", body = ApiErrorBody),
        (status = 503, description = "Service unavailable", body = ApiErrorBody),
        (status = 500, description = "Internal failure", body = ApiErrorBody))
)]
async fn get_credential_requests_id(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<CredentialRequestView>> {
    let operation = { HostOperation::Credential(CredentialOperation::Get { id: resource(id)? }) };
    run(&state, &caller, operation).await
}

#[utoipa::path(post, operation_id = "credentials_post_credential_requests_id_submissions", path = "/credential-requests/{id}/submissions", summary = "Create credential requests submissions", tag = "credentials",
    request_body = CredentialSubmission,
    params(("id" = String, Path, description = "Resource identifier"), ("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = CredentialRequestView),
        (status = 400, description = "Invalid request", body = ApiErrorBody),
        (status = 401, description = "Sign-in required", body = ApiErrorBody),
        (status = 403, description = "Insufficient authority", body = ApiErrorBody),
        (status = 404, description = "Resource unavailable", body = ApiErrorBody),
        (status = 409, description = "Conflicting state", body = ApiErrorBody),
        (status = 412, description = "Revision changed", body = ApiErrorBody),
        (status = 413, description = "Body too large", body = ApiErrorBody),
        (status = 503, description = "Service unavailable", body = ApiErrorBody),
        (status = 500, description = "Internal failure", body = ApiErrorBody))
)]
async fn post_credential_requests_id_submissions(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiPath(id): ApiPath<String>,
    ApiJson(input): ApiJson<CredentialSubmission>,
) -> ApiResult<Json<CredentialRequestView>> {
    let operation = {
        HostOperation::Credential(CredentialOperation::Submit {
            id: resource(id)?,
            input: input,
        })
    };
    run(&state, &caller, operation).await
}

#[utoipa::path(post, operation_id = "credentials_post_credential_requests_id_cancellations", path = "/credential-requests/{id}/cancellations", summary = "Create credential requests cancellations", tag = "credentials",
    request_body = EmptyObject,
    params(("id" = String, Path, description = "Resource identifier"), ("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = CredentialRequestView),
        (status = 400, description = "Invalid request", body = ApiErrorBody),
        (status = 401, description = "Sign-in required", body = ApiErrorBody),
        (status = 403, description = "Insufficient authority", body = ApiErrorBody),
        (status = 404, description = "Resource unavailable", body = ApiErrorBody),
        (status = 409, description = "Conflicting state", body = ApiErrorBody),
        (status = 412, description = "Revision changed", body = ApiErrorBody),
        (status = 413, description = "Body too large", body = ApiErrorBody),
        (status = 503, description = "Service unavailable", body = ApiErrorBody),
        (status = 500, description = "Internal failure", body = ApiErrorBody))
)]
async fn post_credential_requests_id_cancellations(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiPath(id): ApiPath<String>,
    ApiJson(input): ApiJson<EmptyObject>,
) -> ApiResult<Json<CredentialRequestView>> {
    let operation = {
        let _: EmptyObject = input;
        HostOperation::Credential(CredentialOperation::Cancel { id: resource(id)? })
    };
    run(&state, &caller, operation).await
}
