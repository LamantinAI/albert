use crate::{api::host::EventStream, service::auth::AuthService};
use std::{sync::Arc, time::Duration};

use axum::{
    http::HeaderMap, response::sse::Event, response::sse::KeepAlive, response::IntoResponse,
    response::Response, response::Sse,
};
use futures::{stream::unfold, StreamExt};
use tokio::{select, time::interval};

use crate::{
    api::errors::ApiResult, api::host::SessionEventRequest, api::models::EventQuery,
    api::principal::Principal, api::state::ApiState,
};

pub(super) async fn events(
    state: ApiState,
    caller: Principal,
    session_id: String,
    query: EventQuery,
    headers: &HeaderMap,
) -> ApiResult<Response> {
    let request = SessionEventRequest {
        session_id,
        after: headers
            .get("last-event-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
            .or_else(|| query.after),
        runtime_epoch: query.runtime_epoch,
    };
    let events = state.host.events(caller.clone(), request).await?;
    Ok(event_response(state.auth, caller, events))
}
pub(super) fn event_response(
    auth: Arc<AuthService>,
    caller: Principal,
    events: EventStream,
) -> Response {
    let stream = unfold(
        (events, auth, caller, interval(Duration::from_secs(15))),
        |(mut events, auth, caller, mut tick)| async move {
            loop {
                select! {
                    _ = tick.tick() => if auth.validate_principal(&caller).await.is_err() { return None; },
                    next = events.next() => {
                        let event = next?;
                        if auth.validate_principal(&caller).await.is_err() { return None; }
                        let event = event.map(|data| Event::default().id(&data.id).json_data(data).expect("event DTO serializes"));
                        return Some((event, (events, auth, caller, tick)));
                    }
                }
            }
        },
    );
    Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("keepalive"),
        )
        .into_response()
}
