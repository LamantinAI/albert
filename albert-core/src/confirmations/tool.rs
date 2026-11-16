use std::{convert::Infallible, sync::Arc};

use octo_approval::Ingress;
use rig::{completion::ToolDefinition, tool::Tool};
use serde_json::{json, Value};

use super::{ConfirmationGuard, ConfirmationRequest, ConfirmationState, ConfirmationWait};
use crate::{sessions::SessionContext, status::StatusFeed};

#[derive(Clone)]
pub struct RequestConfirmation {
    guard: Arc<ConfirmationGuard>,
    context: SessionContext,
    origin: String,
    ingress: Option<Ingress>,
    feed: StatusFeed,
}
impl RequestConfirmation {
    pub fn new(
        guard: Arc<ConfirmationGuard>,
        context: SessionContext,
        origin: String,
        ingress: Option<Ingress>,
        feed: StatusFeed,
    ) -> Self {
        Self {
            guard,
            context,
            origin,
            ingress,
            feed,
        }
    }
}
impl Tool for RequestConfirmation {
    const NAME: &'static str = "request_confirmation";
    type Error = Infallible;
    type Args = ConfirmationRequest;
    type Output = Value;
    async fn definition(&self, _prompt: String) -> ToolDefinition {
        let limits = self.guard.settings();
        ToolDefinition { name:Self::NAME.into(), description:"Ask the owner for an explicit human decision in the private web approval panel. Describe one concrete action, its effects and optional details without secrets. Stop work after requesting confirmation; a typed Confirmed or Denied receipt will resume the original session at an idle boundary. A conversational confirmation never replaces mandatory SQL/native executor approval, grants permissions or executes the described action.".into(), parameters:json!({"type":"object","properties":{"title":{"type":"string","maxLength":limits.max_title_bytes},"action":{"type":"string","maxLength":limits.max_action_bytes},"effects":{"type":"array","maxItems":limits.max_effects,"items":{"type":"string","maxLength":limits.max_effect_bytes}},"details":{"type":"string","maxLength":limits.max_details_bytes}},"required":["title","action"],"additionalProperties":false}) }
    }
    async fn call(&self, request: ConfirmationRequest) -> Result<Value, Infallible> {
        if !ConfirmationGuard::agent_allowed(&self.context) || !self.context.authorized().await {
            return Ok(
                json!({"error":"Confirmation requires a live private owner conversation with host permissions."}),
            );
        }
        Ok(
            match self.guard.create(
                self.context.clone(),
                self.origin.clone(),
                request,
                self.ingress.clone(),
            ) {
                Ok(view) => {
                    if view.state == ConfirmationState::Pending {
                        self.feed.wait_for_confirmation(ConfirmationWait {
                            request_id: view.id.clone(),
                            title: view.request.title.clone(),
                        });
                    }
                    json!({"request_id":view.id,"status":view.state,"wait_for_user":view.state==ConfirmationState::Pending,"instruction":"The confirmation panel contains the exact question. Wait for its typed decision receipt. This is not an executor authorization."})
                }
                Err(error) => json!({"error":error.to_string()}),
            },
        )
    }
}
