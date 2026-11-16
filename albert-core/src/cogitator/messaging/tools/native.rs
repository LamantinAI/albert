use super::{MessagingAction, SessionMessagingTool};
use rig::{completion::ToolDefinition, tool::Tool};
use serde_json::{json, Value};
use std::convert::Infallible;
impl Tool for SessionMessagingTool {
    const NAME: &'static str = "session_messaging";
    type Error = Infallible;
    type Args = MessagingAction;
    type Output = Value;
    async fn definition(&self, _: String) -> ToolDefinition {
        ToolDefinition { name: Self::NAME.into(), description: "Discover/inspect your durable private sessions with bounded text excerpts (record_id + text_offset continues an oversized record; media/encoded data are omitted), send addressed notices/questions/explicitly bounded work, inspect asynchronous status, cancel or steer one exact message, configure this work's independent intermediate/final report destinations, and inspect/set host execution defaults. Questions have no tools. Notice/Response are passive. Sending returns an acceptance ID immediately; acceptance is not physical delivery. Never claim delivery from acceptance. No temporary exports: Save first. Actor, source, current work and return address are host supplied. Control targets only one message, never unrelated children.".into(),
            parameters: json!({"type":"object","required":["action"],"properties":{
                "action":{"type":"string","enum":["discover","inspect","send","status","cancel","steer","report","reports","execution"]},
                "limit":{"type":"integer"},"before_id":{"type":"integer"},"through_id":{"type":"integer"},
                "record_id":{"type":"integer"},"text_offset":{"type":"integer"},"route_offset":{"type":"integer"},"session_id":{"type":"string"},"message_id":{"type":"string"},"content":{"type":"string"},
                "intent":{"type":"string","enum":["notice","question","work_request"]},"timeout_secs":{"type":"integer"},
                "destination":{"type":"object","description":"{kind:fixed_session,session:UUID,display?:{connector,channel}} or {kind:channel_active,route:{connector,channel}}"},
                "ceiling":{"type":"object","description":"WorkRequest only: {grants:[{connector,kinds:[exact concrete event kinds]}],max_tool_calls,max_output_bytes}. Must be within discovered initiating capabilities."},
                "context":{"type":"array","items":{"type":"object"}},"policy":{"type":"object","description":"On send: revision1 policy atomically binds to new question/work_request. On reports: omit policy to get or set a new revision; optional message_id targets that admitted work, otherwise current native work."},
                "location":{"type":"object","description":"Omit to get; set {machine:host,connector:ID|null}"},"expected_revision":{"type":"integer"}
            },"additionalProperties":false}) }
    }
    async fn call(&self, action: MessagingAction) -> Result<Value, Infallible> {
        Ok(match self.invoke(action).await {
            Ok(value) => value,
            Err(error) => {
                json!({"accepted":false,"status":"not_applied","error":error.to_string()})
            }
        })
    }
}
