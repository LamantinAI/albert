use std::sync::{atomic::AtomicUsize, Arc, Weak};

use octo_approval::Ingress;
use octo_core::CogitatorContext;
use rig::{
    completion::ToolDefinition,
    tool::{ToolDyn, ToolError},
    wasm_compat::WasmBoxedFuture,
};
use serde::Deserialize;
use serde_json::{from_str, json};

use super::{Conversation, Task};
use crate::sessions::SessionContext;
use crate::{cogitator::AlbertCogitator, context::DiagnosticScope};

#[derive(Clone)]
pub struct SubagentTool {
    pub host: Weak<AlbertCogitator>,
    pub ctx: CogitatorContext,
    pub conversation: Conversation,
    pub parent: String,
    pub owner: bool,
    pub spawned: Arc<AtomicUsize>,
    pub ingress: Option<Ingress>,
    pub(crate) session: Option<SessionContext>,
    pub(crate) diagnostics: Option<DiagnosticScope>,
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
            ingress: None,
            session: None,
            diagnostics: None,
        }
    }
    pub(crate) fn with_diagnostics(mut self, scope: Option<DiagnosticScope>) -> Self {
        self.diagnostics = scope;
        self
    }
    pub fn with_ingress(mut self, ingress: Option<Ingress>) -> Self {
        self.ingress = ingress;
        self
    }
    pub(crate) fn with_session(mut self, session: Option<SessionContext>) -> Self {
        self.session = session;
        self
    }
}
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
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
        #[serde(default)]
        limit: Option<usize>,
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
                description: "Delegate a bounded task with explicitly supplied context, whole connector IDs, individual native tool names, and an ordered model subpool (one ID pins a model). First use capabilities to discover grants and configured roles/model IDs. Without configured routing, children use only the current primary model. Omit models/role/effort to use that default. A stage-linked spawn uses the plan assignment; supply stage task_id, stage_id, expected_revision and launch_key, without conflicting overrides. Repeating that same launch key recovers its attempt. Child completion does not verify the stage; check acceptance separately with scratchpad_plan. Spawn returns run_id; use wait to collect its result before answering. Children keep working when the user interrupts you; list recovers their IDs. A child receives no conversation history, owner tools or delegation tool. Default grants are empty. Use the configured default tool-round budget for ordinary research; only lower it deliberately. A partial result is a budget-limited report: inspect its findings and gaps rather than rerunning the whole task. Results return here, never directly to the user unless you explicitly grant a messaging connector. Grant forkd only if its full script/SSH/network authority is needed. Cancelled or failed tools can have UNKNOWN effects; inspect the journal before retrying. inspect returns a bounded inline action index, never the full journal. Its optional limit is capped by configured page size and serialized byte budget; obey returned next_offset/has_more, never assume offset+limit. Read only the selected failed-child entry/field using its read_ref, or wait(seconds:0) for result metadata; do not reconstruct the entire journal or repeat failed external calls. Use read with run_id, entry, part (result or arguments), optional field path (e.g. [result,html]) and character offset/limit to examine selected data. inspect offset is an entry index; read offset is a character index. wait returns the final answer.".into(),
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
                        "models":{"type":"array","items":{"type":"string"},"description":"Omit for the current primary model, or the configured role defaults. Explicit IDs must be allowed by configured routing."},
                        "role":{"type":"string","description":"Optional operator-configured model profile from capabilities; grants no tools."},
                        "reasoning_effort":{"type":"string","description":"Optional effort supported by every selected model; otherwise role/provider default."},
                        "stage":{"type":"object","properties":{
                            "task_id":{"type":"string"},"stage_id":{"type":"string"},
                            "expected_revision":{"type":"integer","minimum":0},
                            "launch_key":{"type":"string","description":"Reuse only to recover this same launch. A deliberate new attempt uses a new key and current revision."}
                        },"required":["task_id","stage_id","expected_revision","launch_key"],"additionalProperties":false},
                        "connectors":{"type":"array","items":{"type":"string"}},
                        "tools":{"type":"array","items":{"type":"string"}},
                        "sql_stores":{"type":"array","items":{"type":"string"},"description":"Explicit SQL store IDs granted to this child; empty by default. Connector grant alone gives no SQL data access."},
                        "sql_write":{"type":"boolean","description":"Allow writes to those SQL stores, false by default; cannot exceed parent rights."},
                        "sql_schema":{"type":"boolean","description":"Allow schema changes to those SQL stores, false by default; cannot exceed parent rights."},
                        "max_tool_turns":{"type":"integer","minimum":1,"description":"Tool-enabled model rounds, not individual tool calls. Omit for the configured default. One tool-free final reporting response is reserved separately."},
                        "timeout_secs":{"type":"integer","minimum":1}
                    },"required":["task"],"additionalProperties":false}
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
