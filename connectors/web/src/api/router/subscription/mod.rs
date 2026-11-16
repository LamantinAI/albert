//! Subscription resource composition.
mod catalog;
mod login;
use utoipa_axum::router::OpenApiRouter;

use crate::api::state::ApiState;

pub(super) fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .merge(login::router())
        .merge(catalog::router())
}
