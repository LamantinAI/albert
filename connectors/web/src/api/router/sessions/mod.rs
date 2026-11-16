//! Sessions resource composition.
mod catalog;
mod deletion;
mod events;
mod messages;
mod starts;
mod work;
use utoipa_axum::router::OpenApiRouter;

use crate::api::state::ApiState;

pub(super) fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .merge(events::router())
        .merge(work::router())
        .merge(starts::router())
        .merge(catalog::router())
        .merge(deletion::router())
        .merge(messages::router())
}
