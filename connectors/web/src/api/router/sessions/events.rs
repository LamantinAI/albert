use axum::{extract::State, http::HeaderMap, response::Response};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::super::{
    actor::BrowserActor, input::resource, input::ApiPath, input::ApiQuery, stream::events,
};
use crate::api::{
    errors::ApiResult, models::ApiErrorBody, models::EventQuery, models::SessionEvent,
    state::ApiState,
};

pub(super) fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new().routes(routes!(subscribe))
}

#[utoipa::path(get, operation_id = "sessions_subscribe", path = "/sessions/{id}/events", summary = "Get sessions events", tag = "sessions", security(("browserCookie" = [])),
    params(("id" = String, Path), ("Last-Event-ID" = Option<String>, Header), EventQuery),
    responses((status = 200, body = SessionEvent, content_type = "text/event-stream"), (status = 400, body = ApiErrorBody), (status = 401, body = ApiErrorBody), (status = 403, body = ApiErrorBody), (status = 404, body = ApiErrorBody), (status = 410, body = ApiErrorBody)))]
async fn subscribe(
    State(state): State<ApiState>,
    BrowserActor(caller): BrowserActor,
    ApiPath(id): ApiPath<String>,
    ApiQuery(query): ApiQuery<EventQuery>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    events(state, caller, resource(id)?, query, &headers).await
}
