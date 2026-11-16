use albert_core::confirmations::{ConfirmationError, ConfirmationState, ConfirmationView};
use serde_json::json;

use super::{handle::ApprovalHandle, native::ApprovalOwner, ApprovalService};
use crate::api::{
    errors::ApiError, errors::ApiResult, models::ApprovalKind, models::ApprovalState,
    models::ApprovalView,
};

impl ApprovalService {
    pub(super) fn confirmation(
        &self,
        owner: &ApprovalOwner,
        id: &str,
        details: bool,
    ) -> ApiResult<ApprovalView> {
        self.runtime
            .sessions
            .confirmations()
            .get(&owner.account, &owner.user, id)
            .map(|value| view(value, details))
            .map_err(error)
    }
    pub(super) async fn decide_confirmation(
        &self,
        owner: &ApprovalOwner,
        id: &str,
        approve: bool,
    ) -> ApiResult<ApprovalView> {
        self.runtime
            .sessions
            .confirmations()
            .decide(&owner.account, &owner.user, id, approve)
            .await
            .map(|value| view(value, false))
            .map_err(error)
    }
}
pub(super) fn view(value: ConfirmationView, details: bool) -> ApprovalView {
    let state = match value.state {
        ConfirmationState::Pending => ApprovalState::Pending,
        ConfirmationState::Confirmed => ApprovalState::Confirmed,
        ConfirmationState::Denied => ApprovalState::Denied,
        ConfirmationState::Cancelled => ApprovalState::Cancelled,
        ConfirmationState::Expired => ApprovalState::Expired,
        ConfirmationState::Superseded => ApprovalState::Superseded,
    };
    ApprovalView {
        id: ApprovalHandle::Confirmation { request: value.id }.encode(),
        connector: "albert".into(),
        kind: ApprovalKind::Confirmation,
        session_id: Some(value.session_id),
        title: value.request.title,
        action: value.request.action,
        effects: value.request.effects,
        details: details
            .then(|| value.request.details.map(|text| json!({"text":text})))
            .flatten(),
        state,
        summary: json!({"kind":"confirmation"}),
        expires_at: Some(value.expires_at),
        can_decide: state == ApprovalState::Pending,
    }
}
fn error(error: ConfirmationError) -> ApiError {
    match error {
        ConfirmationError::Denied => ApiError::forbidden(),
        ConfirmationError::NotFound => ApiError::not_found(),
        ConfirmationError::Conflict => {
            ApiError::conflict("This question already has a different answer.")
        }
        other => ApiError::invalid(other.to_string()),
    }
}
