//! Authenticated binary attachments and playback ranges.
use super::input::IgnoredBody;
use axum::{
    body::Bytes, extract::DefaultBodyLimit, extract::State, http::HeaderMap, response::Response,
};
use serde_json::from_str;
use utoipa_axum::{router::OpenApiRouter, routes};

use super::{actor::BrowserActor, input::resource, input::ApiPath};
use crate::api::{
    errors::ApiResult, media::execute, media::MediaOperation, models::ApiErrorBody,
    models::AttachmentView, models::BinaryContent, models::EmptyObject, principal::Principal,
    state::ApiState,
};

pub(super) fn router(limit: usize) -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(upload))
        .routes(routes!(remove))
        .routes(routes!(read, head))
        .layer(DefaultBodyLimit::max(limit))
}
#[utoipa::path(post, operation_id = "attachments_upload", path = "/attachments", summary = "Create attachments", tag = "attachments", security(("browserCookie" = [])),
    request_body(content = BinaryContent, content_type = "application/octet-stream"),
    params(("x-filename" = Option<String>, Header, description = "JSON-encoded original filename"), ("x-albert-csrf" = String, Header)),
    responses((status = 200, body = AttachmentView), (status = 400, body = ApiErrorBody), (status = 401, body = ApiErrorBody), (status = 403, body = ApiErrorBody), (status = 413, body = ApiErrorBody), (status = 503, body = ApiErrorBody)))]
async fn upload(
    State(state): State<ApiState>,
    BrowserActor(caller): BrowserActor,
    headers: HeaderMap,
    bytes: Bytes,
) -> ApiResult<Response> {
    let filename = headers
        .get("x-filename")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| from_str::<String>(value).ok())
        .unwrap_or_else(|| "attachment".into());
    let mime = headers
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_owned();
    execute(
        &state,
        &caller,
        MediaOperation::Upload {
            bytes: bytes.to_vec(),
            mime,
            filename,
        },
    )
    .await
}
#[utoipa::path(delete, operation_id = "attachments_remove", path = "/attachments/{id}", summary = "Remove attachments", tag = "attachments", security(("browserCookie" = [])), params(("id" = String, Path), ("x-albert-csrf" = String, Header)),
    responses((status = 200, body = EmptyObject), (status = 400, body = ApiErrorBody), (status = 401, body = ApiErrorBody), (status = 403, body = ApiErrorBody), (status = 404, body = ApiErrorBody)))]
async fn remove(
    State(state): State<ApiState>,
    BrowserActor(caller): BrowserActor,
    ApiPath(id): ApiPath<String>,
    _body: IgnoredBody,
) -> ApiResult<Response> {
    execute(
        &state,
        &caller,
        MediaOperation::Delete { id: resource(id)? },
    )
    .await
}
#[utoipa::path(get, operation_id = "attachments_read", path = "/sessions/{id}/attachments/{asset_id}", summary = "Get sessions attachments file", tag = "attachments", security(("browserCookie" = [])), params(("id" = String, Path), ("asset_id" = String, Path), ("Range" = Option<String>, Header)),
    responses((status = 200, body = BinaryContent, content_type = "application/octet-stream"), (status = 206, body = BinaryContent, content_type = "application/octet-stream"), (status = 401, body = ApiErrorBody), (status = 404, body = ApiErrorBody), (status = 410, body = ApiErrorBody), (status = 416, description = "Unsatisfiable range; Content-Range reports complete size")))]
async fn read(
    State(state): State<ApiState>,
    BrowserActor(caller): BrowserActor,
    ApiPath((id, asset_id)): ApiPath<(String, String)>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    file(&state, &caller, id, asset_id, &headers, false).await
}
#[utoipa::path(head, operation_id = "attachments_head", path = "/sessions/{id}/attachments/{asset_id}", summary = "Inspect sessions attachments file", tag = "attachments", security(("browserCookie" = [])), params(("id" = String, Path), ("asset_id" = String, Path), ("Range" = Option<String>, Header)),
    responses((status = 200, description = "Content metadata without a body"), (status = 206, description = "Selected range metadata"), (status = 401, body = ApiErrorBody), (status = 404, body = ApiErrorBody), (status = 410, body = ApiErrorBody), (status = 416, description = "Unsatisfiable range")))]
async fn head(
    State(state): State<ApiState>,
    BrowserActor(caller): BrowserActor,
    ApiPath((id, asset_id)): ApiPath<(String, String)>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    file(&state, &caller, id, asset_id, &headers, true).await
}
async fn file(
    state: &ApiState,
    caller: &Principal,
    id: String,
    asset_id: String,
    headers: &HeaderMap,
    head: bool,
) -> ApiResult<Response> {
    execute(
        state,
        caller,
        MediaOperation::Read {
            session: resource(id)?,
            id: resource(asset_id)?,
            range: headers
                .get("range")
                .map(|value| value.to_str().unwrap_or("").to_owned()),
            head,
        },
    )
    .await
}
