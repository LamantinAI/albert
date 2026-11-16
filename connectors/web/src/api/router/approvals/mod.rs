//! approvals HTTP resources.
use axum::{extract::State, Json};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::{
    actor::OwnerActor, host::run, input::resource, input::ApiJson, input::ApiPath, input::ApiQuery,
};
use crate::api::models::{
    ApiErrorBody, ApprovalDecision, ApprovalList, ApprovalListQuery, ApprovalQuery, ApprovalView,
};
use crate::api::{
    errors::ApiResult, host::ApprovalOperation, host::HostOperation, state::ApiState,
};

pub(super) fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(get_approvals))
        .routes(routes!(get_approval))
        .routes(routes!(post_approvals_id_decisions))
}

#[utoipa::path(get, operation_id = "approvals_get_approvals", path = "/approvals", summary = "Get approvals", tag = "approvals", params(ApprovalListQuery),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = ApprovalList),
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
async fn get_approvals(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiQuery(query): ApiQuery<ApprovalListQuery>,
) -> ApiResult<Json<ApprovalList>> {
    let operation = HostOperation::Approval(ApprovalOperation::Filtered(query));
    run(&state, &caller, operation).await
}

#[utoipa::path(post, operation_id = "approvals_post_approvals_id_decisions", path = "/approvals/{id}/decisions", summary = "Create approvals decisions", tag = "approvals",
    request_body = ApprovalDecision,
    params(("id" = String, Path, description = "Resource identifier"), ("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = ApprovalView),
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
async fn post_approvals_id_decisions(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiPath(id): ApiPath<String>,
    ApiJson(input): ApiJson<ApprovalDecision>,
) -> ApiResult<Json<ApprovalView>> {
    let operation = {
        HostOperation::Approval(ApprovalOperation::Decide {
            id: resource(id)?,
            input: input,
        })
    };
    run(&state, &caller, operation).await
}

#[utoipa::path(get,operation_id="approvals_get_approval",path="/approvals/{id}",summary="Get protected action status",tag="approvals",
 params(("id"=String,Path,description="Opaque approval resource"),ApprovalQuery), security(("browserCookie"=[])),
 responses((status=200,body=ApprovalView),(status=401,body=ApiErrorBody),(status=403,body=ApiErrorBody),(status=404,body=ApiErrorBody),(status=503,body=ApiErrorBody))) ]
async fn get_approval(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiPath(id): ApiPath<String>,
    ApiQuery(query): ApiQuery<ApprovalQuery>,
) -> ApiResult<Json<ApprovalView>> {
    run(
        &state,
        &caller,
        HostOperation::Approval(ApprovalOperation::Get {
            id: resource(id)?,
            details: query.include_details,
        }),
    )
    .await
}
