use std::{collections::HashMap, convert::Infallible};

use octo_core::EventKind;
use octo_rig::{DispatchArgs, OctoDispatchTool};
use rig::{completion::ToolDefinition, tool::Tool};
use serde_json::{json, Value};

use crate::{cogitator::session_dispatch::pin_alarm, sessions::SessionContext};

/// Enforce the same whole-connector grant used to build discovery. Never publish
/// a rejected call, even if the model guesses a hidden target's name.
pub struct ScopedDispatch {
    pub inner: OctoDispatchTool,
    pub allowed: HashMap<String, Vec<EventKind>>,
    pub(crate) session: Option<SessionContext>,
}
impl Tool for ScopedDispatch {
    const NAME: &'static str = "dispatch_to_connector";
    type Error = Infallible;
    type Args = DispatchArgs;
    type Output = Value;
    async fn definition(&self, prompt: String) -> ToolDefinition {
        self.inner.definition(prompt).await
    }
    async fn call(&self, mut args: DispatchArgs) -> Result<Value, Infallible> {
        // Runtime control is not a connector capability: the bus control listener
        // consumes these envelopes regardless of their target.
        if args.kind.starts_with("octo.control.") {
            return Ok(json!({"error":"Runtime control is not delegated.","status":"not_sent"}));
        }
        let Some(accepts) = self.allowed.get(&args.target) else {
            return Ok(
                json!({"error":"Connector not granted to this subagent.","status":"not_sent"}),
            );
        };
        let kind = EventKind::new(&args.kind);
        if !accepts.iter().any(|pattern| kind.matches(pattern.as_str())) {
            return Ok(
                json!({"error":"Event kind is not an input of this connector.","status":"not_sent"}),
            );
        }
        if let Err(error) = pin_alarm(&mut args, self.session.as_ref()) {
            return Ok(error);
        }
        self.inner.call(args).await
    }
}
