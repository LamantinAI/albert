use std::{collections::HashMap, time::Duration};

use axum::{
    http::HeaderMap,
    response::{
        sse::{Event, KeepAlive},
        IntoResponse, Response, Sse,
    },
};
use futures::{stream::unfold, StreamExt};
use tokio::{select, time::interval};

use super::id;
use crate::api::{
    errors::{ApiError, ApiResult},
    extractors::cookie_token,
    host::SessionEventRequest,
    principal::Principal,
    state::ApiState,
};

pub(super) async fn events(
    state: ApiState,
    caller: Principal,
    params: &HashMap<String, String>,
    query: &HashMap<String, String>,
    headers: &HeaderMap,
) -> ApiResult<Response> {
    let token = cookie_token(headers).ok_or_else(ApiError::unauthorized)?;
    let request = SessionEventRequest {
        session_id: id(params, "id")?,
        after: headers
            .get("last-event-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
            .or_else(|| query.get("after").cloned()),
        runtime_epoch: query.get("runtime_epoch").cloned(),
    };
    let events = state.host.events(caller, request).await?;
    let stream = unfold(
        (events, state.auth, token, interval(Duration::from_secs(15))),
        |(mut events, auth, token, mut tick)| async move {
            loop {
                select! {
                    _ = tick.tick() => if auth.authenticate(&token).await.is_err() { return None; },
                    next = events.next() => {
                        let event = next?;
                        if auth.authenticate(&token).await.is_err() { return None; }
                        let event = event.map(|data| Event::default().id(&data.id).json_data(data).expect("event DTO serializes"));
                        return Some((event, (events, auth, token, tick)));
                    }
                }
            }
        },
    );
    Ok(Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("keepalive"),
        )
        .into_response())
}
