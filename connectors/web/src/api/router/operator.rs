use axum::{extract::State, response::Response, Json};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::{actor::BrowserActor, host::run, input::ApiQuery, stream::event_response};
use crate::api::{
    errors::ApiResult, host::HostOperation, models::ApiErrorBody, models::ModelUsageSummary,
    models::OperatorQuery, models::OperatorSnapshot, models::SessionEvent,
    models::SubscriptionLimitsView, state::ApiState,
};

pub(super) fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(snapshot))
        .routes(routes!(events))
        .routes(routes!(usage))
        .routes(routes!(limits))
}

#[utoipa::path(get,operation_id="operator_snapshot",path="/operator/snapshot",tag="operator",security(("browserCookie"=[])),params(OperatorQuery),responses((status=200,body=OperatorSnapshot),(status=400,body=ApiErrorBody),(status=403,body=ApiErrorBody)))]
async fn snapshot(
    State(state): State<ApiState>,
    BrowserActor(caller): BrowserActor,
    ApiQuery(query): ApiQuery<OperatorQuery>,
) -> ApiResult<Json<OperatorSnapshot>> {
    run(&state, &caller, HostOperation::Operator(query)).await
}

#[utoipa::path(get,operation_id="operator_events",path="/operator/events",tag="operator",security(("browserCookie"=[])),params(OperatorQuery),responses((status=200,body=SessionEvent,content_type="text/event-stream"),(status=403,body=ApiErrorBody)))]
async fn events(
    State(state): State<ApiState>,
    BrowserActor(caller): BrowserActor,
    ApiQuery(query): ApiQuery<OperatorQuery>,
) -> ApiResult<Response> {
    let events = state.host.operator_events(caller.clone(), query).await?;
    Ok(event_response(state.auth, caller, events))
}
#[utoipa::path(get,operation_id="model_usage",path="/model-usage",tag="models",security(("browserCookie"=[])),responses((status=200,body=ModelUsageSummary),(status=403,body=ApiErrorBody)))]
async fn usage(
    State(state): State<ApiState>,
    BrowserActor(caller): BrowserActor,
) -> ApiResult<Json<ModelUsageSummary>> {
    run(&state, &caller, HostOperation::ModelUsage).await
}
#[utoipa::path(get,operation_id="subscription_limits",path="/openai-subscription/limits",tag="subscription",security(("browserCookie"=[])),responses((status=200,body=SubscriptionLimitsView),(status=403,body=ApiErrorBody)))]
async fn limits(
    State(state): State<ApiState>,
    BrowserActor(caller): BrowserActor,
) -> ApiResult<Json<SubscriptionLimitsView>> {
    run(&state, &caller, HostOperation::SubscriptionLimits).await
}
