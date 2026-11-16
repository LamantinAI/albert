use axum::{extract::State, Json};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::{actor::OwnerActor, host::run, input::ApiJson};
use crate::api::{
    errors::ApiResult, host::HostOperation, memory::MemoryInitialization, memory::MemoryOperation,
    memory::MemoryView, memory::MemoryWrite, models::ApiErrorBody, state::ApiState,
};

pub(super) fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(get))
        .routes(routes!(put))
        .routes(routes!(initialize))
}
#[utoipa::path(get, operation_id = "memory_get", path = "/memory", summary = "Get memory settings and initialization status", tag = "memory",
 security(("browserCookie" = [])), responses((status = 200, body = MemoryView), (status = 401, body = ApiErrorBody), (status = 403, body = ApiErrorBody), (status = 400, body = ApiErrorBody)))]
async fn get(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
) -> ApiResult<Json<MemoryView>> {
    run(&state, &caller, HostOperation::Memory(MemoryOperation::Get)).await
}
#[utoipa::path(put, operation_id = "memory_put", path = "/memory", summary = "Save memory selection", tag = "memory",
 request_body = MemoryWrite, params(("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
 security(("browserCookie" = [])), responses((status = 200, description = "Saved settings and truthful runtime status; initialization can fail without discarding settings", body = MemoryView), (status = 400, body = ApiErrorBody), (status = 401, body = ApiErrorBody), (status = 403, body = ApiErrorBody), (status = 409, body = ApiErrorBody), (status = 412, body = ApiErrorBody), (status = 503, body = ApiErrorBody)))]
async fn put(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiJson(input): ApiJson<MemoryWrite>,
) -> ApiResult<Json<MemoryView>> {
    run(
        &state,
        &caller,
        HostOperation::Memory(MemoryOperation::Put(input)),
    )
    .await
}
#[utoipa::path(post, operation_id = "memory_initializations_create", path = "/memory/initializations", summary = "Retry saved memory initialization", tag = "memory",
 description = "Initialization can create the Albert initiative. Existing active memory is retained until an explicit process restart.",
 request_body = MemoryInitialization, params(("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
 security(("browserCookie" = [])), responses((status = 200, body = MemoryView), (status = 400, body = ApiErrorBody), (status = 401, body = ApiErrorBody), (status = 403, body = ApiErrorBody), (status = 409, body = ApiErrorBody), (status = 412, body = ApiErrorBody), (status = 503, body = ApiErrorBody)))]
async fn initialize(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiJson(input): ApiJson<MemoryInitialization>,
) -> ApiResult<Json<MemoryView>> {
    run(
        &state,
        &caller,
        HostOperation::Memory(MemoryOperation::Initialize(input)),
    )
    .await
}
