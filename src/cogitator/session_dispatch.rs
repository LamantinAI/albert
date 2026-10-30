use std::convert::Infallible;

use octo_rig::{DispatchArgs, OctoDispatchTool};
use rig::{completion::ToolDefinition, tool::Tool};
use serde_json::{json, Value};

use super::messaging::tools::SessionMessagingTool;
use crate::{
    messaging::contract::{ExecutionLocation, Machine},
    sessions::SessionContext,
};

pub(super) struct SessionDispatch {
    pub inner: OctoDispatchTool,
    pub session: Option<SessionContext>,
    pub messaging: Option<SessionMessagingTool>,
}

impl Tool for SessionDispatch {
    const NAME: &'static str = "dispatch_to_connector";
    type Error = Infallible;
    type Args = DispatchArgs;
    type Output = Value;
    async fn definition(&self, prompt: String) -> ToolDefinition {
        self.inner.definition(prompt).await
    }
    async fn call(&self, mut args: DispatchArgs) -> Result<Value, Infallible> {
        if let Err(error) = pin_alarm(&mut args, self.session.as_ref()) {
            return Ok(error);
        }
        self.inner
            .clone()
            .with_context(ExecutionLocation {
                machine: Machine::Host,
                connector: Some(args.target.clone()),
            })
            .call(args)
            .await
    }
}

pub(crate) fn pin_alarm(
    args: &mut DispatchArgs,
    session: Option<&SessionContext>,
) -> Result<(), Value> {
    if let Some(session) = session {
        if args.kind == "octo.scheduler.add_alarm" {
            let Some(command) = args.payload.as_object_mut() else {
                return Err(
                    json!({"error":"Alarm command must be an object.","status":"not_sent"}),
                );
            };
            let payload = command.entry("payload").or_insert_with(|| json!({}));
            let Some(payload) = payload.as_object_mut() else {
                return Err(
                    json!({"error":"Alarm payload must be an object.","status":"not_sent"}),
                );
            };
            payload.remove("routine");
            payload.insert("albert_session_id".into(), json!(session.session_id()));
            payload.insert("albert_session_host".into(), json!("host"));
            payload.insert("reply_via".into(), json!(session.reply_route().connector()));
            payload.insert("channel".into(), json!(session.reply_route().channel()));
        }
    }
    Ok(())
}
