//! A model can request a name, but cannot submit a credential through this tool.
use std::{convert::Infallible, sync::Arc};

use octo_approval::Ingress;
use octo_history::SessionId;
use rig::{completion::ToolDefinition, tool::Tool};
use serde::Deserialize;
use serde_json::{json, Value};

use super::{CredentialGuard, CredentialState};
use crate::sessions::SessionContext;

#[derive(Clone)]
pub struct RequestCredential {
    guard: Arc<CredentialGuard>,
    context: SessionContext,
    scope: String,
    ingress: Option<Ingress>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CredentialName {
    pub name: String,
}
impl RequestCredential {
    pub fn new(
        guard: Arc<CredentialGuard>,
        context: SessionContext,
        scope: String,
        ingress: Option<Ingress>,
    ) -> Self {
        Self {
            guard,
            context,
            scope,
            ingress,
        }
    }
}
impl Tool for RequestCredential {
    const NAME: &'static str = "request_credential";
    type Error = Infallible;
    type Args = CredentialName;
    type Output = Value;
    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition{name:Self::NAME.into(),description:"Ask the owner to enter a server credential in the dedicated web form. Supply only the suggested environment-variable name, never a secret value. The owner may edit the name. Do not ask for the value in chat or read it back. Return to the user and wait; a name-only saved receipt will continue the session at an idle boundary. Saved credentials may still need the usual configuration apply/restart.".into(),parameters:json!({"type":"object","properties":{"name":{"type":"string","maxLength":128}},"required":["name"],"additionalProperties":false})}
    }
    async fn call(&self, args: Self::Args) -> Result<Value, Infallible> {
        if !CredentialGuard::agent_allowed(&self.context) || !self.context.authorized().await {
            return Ok(
                json!({"error":"Credential requests require a live private owner conversation with host permissions."}),
            );
        }
        let account = self.context.account().expect("owner checked");
        let result = self.guard.create(
            &account.account_id,
            &account.user_id,
            SessionId::new().as_str(),
            &args.name,
            Some(self.context.clone()),
            Some(self.scope.clone()),
        );
        Ok(match result {
            Ok(request) => {
                self.guard.attach_ingress(&request.id, self.ingress.clone());
                if request.state == CredentialState::Pending {
                    json!({"request_id":request.id,"name":request.suggested_name,"status":"awaiting_user","instruction":"Use the credential panel in the authorized web conversation. Do not send the value as a message."})
                } else {
                    json!({"request_id":request.id,"name":request.name.unwrap_or(request.suggested_name),"status":request.state,"instruction":"This request is already terminal. Do not ask for the value again in this turn."})
                }
            }
            Err(error) => json!({"error":error.to_string()}),
        })
    }
}
