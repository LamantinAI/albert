//! Configuration resource composition.
mod applications;
mod documents;
mod prompts;
mod secrets;
use utoipa_axum::router::OpenApiRouter;

use crate::api::state::ApiState;

pub(super) fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .merge(documents::router())
        .merge(applications::router())
        .merge(prompts::router())
        .merge(secrets::router())
}
