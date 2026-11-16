//! Owner-managed native MCP sessions, independent of envelope connectors.
use super::{
    actor::OwnerActor, host::run, input::resource, input::revision, input::ApiJson, input::ApiPath,
    input::ApiQuery, input::IgnoredBody,
};
use crate::api::{
    errors::ApiResult, host::HostOperation, mcp::McpOperation, models::ApiErrorBody,
    models::McpServerList, models::McpServerWrite, models::McpToolList, models::McpToolQuery,
    state::ApiState,
};
use axum::{extract::State, http::HeaderMap, Json};
use utoipa_axum::{router::OpenApiRouter, routes};
pub(super) fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(list))
        .routes(routes!(create))
        .routes(routes!(update))
        .routes(routes!(remove))
        .routes(routes!(connect))
        .routes(routes!(discover))
        .routes(routes!(tools))
}
#[utoipa::path(get,operation_id="mcp_list",path="/mcp-servers",tag="mcp",security(("browserCookie"=[])),responses((status=200,body=McpServerList),(status=400,body=ApiErrorBody),(status=401,body=ApiErrorBody),(status=403,body=ApiErrorBody),(status=404,body=ApiErrorBody),(status=409,body=ApiErrorBody),(status=412,body=ApiErrorBody),(status=503,body=ApiErrorBody)))]
async fn list(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
) -> ApiResult<Json<McpServerList>> {
    run(&state, &caller, HostOperation::Mcp(McpOperation::List)).await
}

#[utoipa::path(post,operation_id="mcp_create",path="/mcp-servers",tag="mcp",security(("browserCookie"=[])),request_body=McpServerWrite,params(("x-albert-csrf"=String,Header,description="Current browser CSRF value")),responses((status=200,body=McpServerList),(status=400,body=ApiErrorBody),(status=401,body=ApiErrorBody),(status=403,body=ApiErrorBody),(status=404,body=ApiErrorBody),(status=409,body=ApiErrorBody),(status=412,body=ApiErrorBody),(status=503,body=ApiErrorBody)))]
async fn create(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiJson(input): ApiJson<McpServerWrite>,
) -> ApiResult<Json<McpServerList>> {
    run(
        &state,
        &caller,
        HostOperation::Mcp(McpOperation::Create(input)),
    )
    .await
}

#[utoipa::path(put,operation_id="mcp_update",path="/mcp-servers/{id}",tag="mcp",security(("browserCookie"=[])),request_body=McpServerWrite,params(("id"=String,Path,description="Configured server ID"),("x-albert-csrf"=String,Header,description="Current browser CSRF value")),responses((status=200,body=McpServerList),(status=400,body=ApiErrorBody),(status=401,body=ApiErrorBody),(status=403,body=ApiErrorBody),(status=404,body=ApiErrorBody),(status=409,body=ApiErrorBody),(status=412,body=ApiErrorBody),(status=503,body=ApiErrorBody)))]
async fn update(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiPath(id): ApiPath<String>,
    ApiJson(input): ApiJson<McpServerWrite>,
) -> ApiResult<Json<McpServerList>> {
    run(
        &state,
        &caller,
        HostOperation::Mcp(McpOperation::Update {
            id: resource(id)?,
            input,
        }),
    )
    .await
}

#[utoipa::path(delete,operation_id="mcp_remove",path="/mcp-servers/{id}",tag="mcp",security(("browserCookie"=[])),params(("id"=String,Path,description="Configured server ID"),("If-Match"=String,Header,description="Exact registry revision"),("x-albert-csrf"=String,Header,description="Current browser CSRF value")),responses((status=200,body=McpServerList),(status=400,body=ApiErrorBody),(status=401,body=ApiErrorBody),(status=403,body=ApiErrorBody),(status=404,body=ApiErrorBody),(status=409,body=ApiErrorBody),(status=412,body=ApiErrorBody),(status=503,body=ApiErrorBody)))]
async fn remove(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiPath(id): ApiPath<String>,
    headers: HeaderMap,
    _body: IgnoredBody,
) -> ApiResult<Json<McpServerList>> {
    run(
        &state,
        &caller,
        HostOperation::Mcp(McpOperation::Delete {
            id: resource(id)?,
            revision: revision(&headers)?,
        }),
    )
    .await
}

#[utoipa::path(post,operation_id="mcp_connect",path="/mcp-servers/{id}/connections",tag="mcp",security(("browserCookie"=[])),params(("id"=String,Path,description="Configured server ID"),("x-albert-csrf"=String,Header,description="Current browser CSRF value")),responses((status=200,body=McpServerList),(status=400,body=ApiErrorBody),(status=401,body=ApiErrorBody),(status=403,body=ApiErrorBody),(status=404,body=ApiErrorBody),(status=409,body=ApiErrorBody),(status=412,body=ApiErrorBody),(status=503,body=ApiErrorBody)))]
async fn connect(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiPath(id): ApiPath<String>,
    _body: IgnoredBody,
) -> ApiResult<Json<McpServerList>> {
    run(
        &state,
        &caller,
        HostOperation::Mcp(McpOperation::Reconnect { id: resource(id)? }),
    )
    .await
}

#[utoipa::path(post,operation_id="mcp_discover",path="/mcp-servers/{id}/tool-discoveries",tag="mcp",security(("browserCookie"=[])),params(("id"=String,Path,description="Configured server ID"),("x-albert-csrf"=String,Header,description="Current browser CSRF value")),responses((status=200,body=McpServerList),(status=400,body=ApiErrorBody),(status=401,body=ApiErrorBody),(status=403,body=ApiErrorBody),(status=404,body=ApiErrorBody),(status=409,body=ApiErrorBody),(status=412,body=ApiErrorBody),(status=503,body=ApiErrorBody)))]
async fn discover(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiPath(id): ApiPath<String>,
    _body: IgnoredBody,
) -> ApiResult<Json<McpServerList>> {
    run(
        &state,
        &caller,
        HostOperation::Mcp(McpOperation::Refresh { id: resource(id)? }),
    )
    .await
}

#[utoipa::path(get,operation_id="mcp_tools",path="/mcp-servers/{id}/tools",tag="mcp",security(("browserCookie"=[])),params(("id"=String,Path,description="Configured server ID"),McpToolQuery),responses((status=200,body=McpToolList),(status=400,body=ApiErrorBody),(status=401,body=ApiErrorBody),(status=403,body=ApiErrorBody),(status=404,body=ApiErrorBody),(status=409,body=ApiErrorBody),(status=412,body=ApiErrorBody),(status=503,body=ApiErrorBody)))]
async fn tools(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiPath(id): ApiPath<String>,
    ApiQuery(query): ApiQuery<McpToolQuery>,
) -> ApiResult<Json<McpToolList>> {
    run(
        &state,
        &caller,
        HostOperation::Mcp(McpOperation::Tools {
            id: resource(id)?,
            query,
        }),
    )
    .await
}
