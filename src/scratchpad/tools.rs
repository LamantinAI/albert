use std::{convert::Infallible, sync::Arc};

use rig::{completion::ToolDefinition, tool::Tool};
use serde::Deserialize;
use serde_json::{json, Value};

use super::{PadScope, PlanArgs, ScratchpadStore};

/// A scratchpad handle bound to one channel. Builds the rig tools.
#[derive(Clone)]
pub struct PadHandle {
    store: Arc<ScratchpadStore>,
    scope: PadScope,
}

impl PadHandle {
    pub(super) fn new(store: Arc<ScratchpadStore>, scope: PadScope) -> Self {
        Self { store, scope }
    }
    pub fn plan(&self) -> Plan {
        Plan(self.clone())
    }
    pub fn goal(&self) -> Goal {
        Goal(self.clone())
    }
    pub fn step(&self) -> Step {
        Step(self.clone())
    }
    pub fn mark(&self) -> Mark {
        Mark(self.clone())
    }
    pub fn note(&self) -> Note {
        Note(self.clone())
    }
    pub fn clear(&self) -> Clear {
        Clear(self.clone())
    }
}

#[derive(Debug, Deserialize)]
pub struct GoalArgs {
    pub goal: String,
}
#[derive(Debug, Deserialize)]
pub struct StepArgs {
    pub text: String,
}
#[derive(Debug, Deserialize)]
pub struct MarkArgs {
    pub step: usize,
    pub status: String,
    #[serde(default)]
    pub note: Option<String>,
}
#[derive(Debug, Deserialize)]
pub struct NoteArgs {
    pub text: String,
}
#[derive(Debug, Default, Deserialize)]
pub struct ClearArgs {}

/// Generate a rig `Tool` over a [`PadHandle`]; the body is an expression over
/// `store: &ScratchpadStore`, `scope: &PadScope` and the deserialized `args`.
macro_rules! pad_tool {
    ($tool:ident, $name:literal, $desc:expr, $args:ty, $params:tt, |$store:ident, $channel:ident, $args_id:ident| $body:expr) => {
        #[derive(Clone)]
        pub struct $tool(PadHandle);

        impl Tool for $tool {
            const NAME: &'static str = $name;
            type Error = Infallible;
            type Args = $args;
            type Output = Value;

            async fn definition(&self, _prompt: String) -> ToolDefinition {
                ToolDefinition {
                    name: $name.to_string(),
                    description: ($desc).to_string(),
                    parameters: json!($params),
                }
            }

            async fn call(&self, $args_id: $args) -> Result<Value, Infallible> {
                let $store = &*self.0.store;
                let $channel = &self.0.scope;
                Ok($body)
            }
        }
    };
}

pad_tool!(
    Goal,
    "scratchpad_goal",
    "Start or replace the working scratchpad for the current multi-step task with a one-line goal (clears any prior steps). Use for tasks with more than one step.",
    GoalArgs,
    { "type": "object", "properties": { "goal": { "type": "string" } }, "required": ["goal"] },
    |store, channel, a| store.set_goal(channel, a.goal)
);

pad_tool!(
    Step,
    "scratchpad_step",
    "Add a subtask to the scratchpad (status pending). Returns its number.",
    StepArgs,
    { "type": "object", "properties": { "text": { "type": "string" } }, "required": ["text"] },
    |store, channel, a| store.add_step(channel, a.text)
);

pad_tool!(
    Mark,
    "scratchpad_mark",
    "Set a subtask's status: pending|in_progress|done|verified|blocked. Mark a step verified only after you actually checked it; the task is finished only when every step is verified.",
    MarkArgs,
    { "type": "object", "properties": { "step": { "type": "integer", "description": "1-based step number" }, "status": { "type": "string" }, "note": { "type": "string" } }, "required": ["step", "status"] },
    |store, channel, a| store.mark(channel, a.step, &a.status, a.note)
);

pad_tool!(
    Note,
    "scratchpad_note",
    "Record a finding, result, or decision on the scratchpad.",
    NoteArgs,
    { "type": "object", "properties": { "text": { "type": "string" } }, "required": ["text"] },
    |store, channel, a| store.note(channel, a.text)
);

pad_tool!(
    Clear,
    "scratchpad_clear",
    "Clear the scratchpad when the task is finished or abandoned. Save anything durable to memory (kaeru) first.",
    ClearArgs,
    { "type": "object", "properties": {} },
    |store, channel, _a| store.clear(channel)
);

pad_tool!(
    Plan,
    "scratchpad_plan",
    "Read or mutate stable stages. get returns a paged summary; get_stage returns full stage attempts/evidence. add/update/mark require task_id and expected_revision. Add requires text/spec; update replaces text/spec or note. Mark uses pending|in_progress|done|verified|blocked; planned verification requires evidence for each zero-based acceptance criterion. IDs are host generated. scope is descriptive; assignment requests are resolved by the host.",
    PlanArgs,
    {
        "type":"object",
        "properties": {
            "action":{"type":"string","enum":["get","get_stage","add","update","mark"]},
            "task_id":{"type":"string"}, "expected_revision":{"type":"integer","minimum":0},
            "stage_id":{"type":"string"}, "text":{"type":"string"},
            "spec": {
                "type":"object", "properties": {
                    "depends_on":{"type":"array","items":{"type":"string"}},
                    "assignment":{"type":"object","properties":{
                        "role":{"type":"string"},
                        "models":{"type":"array","items":{"type":"string"}},
                        "reasoning_effort":{"type":"string"}, "why":{"type":"string"}
                    }},
                    "acceptance":{"type":"array","items":{"type":"string"}},
                    "scope":{"type":"array","items":{"type":"string"}}
                }
            },
            "status":{"type":"string"}, "note":{"type":"string"},
            "evidence":{"type":"array","items":{"type":"object","properties":{
                "criterion":{"type":"integer","minimum":0},
                "reference":{"type":"string"}, "summary":{"type":"string"}
            },"required":["criterion","reference","summary"]}},
            "offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1}
        },
        "required":["action"]
    },
    |store, scope, args| store.plan_action(scope, args)
);
