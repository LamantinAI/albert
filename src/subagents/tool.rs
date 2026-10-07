use std::sync::{atomic::AtomicUsize, Arc, Weak};

use octo_core::CogitatorContext;
use rig::{
    completion::ToolDefinition,
    tool::{ToolDyn, ToolError},
    wasm_compat::WasmBoxedFuture,
};
use serde::Deserialize;
use serde_json::{from_str, json};

use super::{Conversation, Task};
use crate::cogitator::AlbertCogitator;

#[derive(Clone)]
pub struct SubagentTool {
    pub host: Weak<AlbertCogitator>,
    pub ctx: CogitatorContext,
    pub conversation: Conversation,
    pub parent: String,
    pub owner: bool,
    pub spawned: Arc<AtomicUsize>,
}
impl SubagentTool {
    pub fn new(
        host: Weak<AlbertCogitator>,
        ctx: CogitatorContext,
        conversation: Conversation,
        parent: String,
        owner: bool,
    ) -> Self {
        Self {
            host,
            ctx,
            conversation,
            parent,
            owner,
            spawned: Arc::new(AtomicUsize::new(0)),
        }
    }
}
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Args {
    Capabilities,
    Spawn {
        task: Task,
    },
    List,
    Inspect {
        run_id: String,
        #[serde(default)]
        offset: usize,
    },
    Read {
        run_id: String,
        entry: usize,
        #[serde(default = "result_part")]
        part: String,
        #[serde(default)]
        field: Vec<String>,
        #[serde(default)]
        offset: usize,
        #[serde(default)]
        limit: Option<usize>,
    },
    Wait {
        run_id: String,
        #[serde(default = "wait_default")]
        seconds: u64,
    },
    Cancel {
        run_id: String,
    },
}
fn result_part() -> String {
    "result".into()
}
fn wait_default() -> u64 {
    30
}

// Type erasure also breaks the root-tool -> child runner -> provider builder
// type cycle. Children never receive this tool.
impl ToolDyn for SubagentTool {
    fn name(&self) -> String {
        "subagent".into()
    }
    fn definition(&self, _: String) -> WasmBoxedFuture<'_, ToolDefinition> {
        Box::pin(async {
            ToolDefinition { name: "subagent".into(),
                description: "Delegate a bounded task with explicitly supplied context, whole connector IDs, individual native tool names, and an ordered model subpool (one ID pins a model). First use capabilities to discover grants and model IDs. Spawn returns run_id; use wait to collect its result before answering. Children keep working when the user interrupts you; list recovers their IDs. A child receives no conversation history, owner tools or delegation tool. Default grants are empty. Results return here, never directly to the user unless you explicitly grant a messaging connector. Grant forkd only if its full script/SSH/network authority is needed. Cancelled or failed tools can have UNKNOWN effects; inspect the journal before retrying. inspect returns a paginated action index and paths to heavy payload files, never the full journal. Use read with run_id, entry, part (result or arguments), optional field path (e.g. [result,html]) and character offset/limit to examine selected data. inspect offset is an entry index; read offset is a character index. wait returns the final answer.".into(),
                parameters: json!({"type":"object","properties":{
                    "action":{"type":"string","enum":["capabilities","spawn","list","inspect","read","wait","cancel"]},
                    "entry":{"type":"integer","minimum":0},
                    "part":{"type":"string","enum":["arguments","result"]},
                    "field":{"type":"array","items":{"type":"string"}},
                    "offset":{"type":"integer","minimum":0},
                    "limit":{"type":"integer","minimum":1},
                    "run_id":{"type":"string"},"seconds":{"type":"integer","minimum":0,"maximum":60},
                    "task":{"type":"object","properties":{
                        "task":{"type":"string"},"context":{"type":"string"},
                        "models":{"type":"array","items":{"type":"string"},"minItems":1},
                        "connectors":{"type":"array","items":{"type":"string"}},
                        "tools":{"type":"array","items":{"type":"string"}},
                        "max_tool_turns":{"type":"integer","minimum":1},
                        "timeout_secs":{"type":"integer","minimum":1}
                    },"required":["task","models"],"additionalProperties":false}
                },"required":["action"],"additionalProperties":false}) }
        })
    }
    fn call(&self, args: String) -> WasmBoxedFuture<'_, Result<String, ToolError>> {
        Box::pin(async move {
            let args: Args = from_str(&args).map_err(ToolError::JsonError)?;
            let Some(host) = self.host.upgrade() else {
                return Ok(json!({"error":"Albert is shutting down."}).to_string());
            };
            Ok(match host.subagent_command(self, args).await {
                Ok(value) => value,
                Err(error) => json!({"error":error}),
            }
            .to_string())
        })
    }
}
