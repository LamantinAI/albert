//! Loop scratchpad — a super-operational, per-task working object the agent
//! authors and sees each turn. Distinct from the chat transcript (a log) and from
//! kaeru (durable memory, a tool): this is ephemeral task state that makes
//! multi-step tasks verifiable — explicit subtasks with statuses, so "done" means
//! "all verified", not "the model felt finished".
//!
//! One active scratchpad per channel (the current task in that conversation),
//! kept in memory (lost on restart, by design — consolidate durable итоги to
//! kaeru first). The agent mutates it via the tools below; the cogitator renders
//! the current state into the preamble every turn.

use std::{
    collections::HashMap,
    convert::Infallible,
    sync::{Arc, Mutex},
};

use rig::{completion::ToolDefinition, tool::Tool};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::debug;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Pending,
    InProgress,
    Done,
    Verified,
    Blocked,
}

impl Status {
    fn parse(s: &str) -> Option<Status> {
        match s.trim().to_ascii_lowercase().as_str() {
            "pending" => Some(Status::Pending),
            "in_progress" | "in-progress" | "in progress" => Some(Status::InProgress),
            "done" => Some(Status::Done),
            "verified" => Some(Status::Verified),
            "blocked" => Some(Status::Blocked),
            _ => None,
        }
    }
    fn as_str(self) -> &'static str {
        match self {
            Status::Pending => "pending",
            Status::InProgress => "in_progress",
            Status::Done => "done",
            Status::Verified => "verified",
            Status::Blocked => "blocked",
        }
    }
}

struct Item {
    text: String,
    status: Status,
    note: Option<String>,
}

#[derive(Default)]
struct Scratchpad {
    goal: Option<String>,
    steps: Vec<Item>,
    notes: Vec<String>,
}

impl Scratchpad {
    fn is_empty(&self) -> bool {
        self.goal.is_none() && self.steps.is_empty() && self.notes.is_empty()
    }

    fn render(&self) -> String {
        let mut s = String::from("Scratchpad (current task — keep it updated; the task is done only when every step is verified):\n");
        s += &format!("Goal: {}\n", self.goal.as_deref().unwrap_or("(none set)"));
        if self.steps.is_empty() {
            s += "Steps: (none)\n";
        } else {
            s += "Steps:\n";
            for (i, step) in self.steps.iter().enumerate() {
                s += &format!("  {}. [{}] {}", i + 1, step.status.as_str(), step.text);
                if let Some(n) = &step.note {
                    s += &format!(" — {n}");
                }
                s += "\n";
            }
        }
        if !self.notes.is_empty() {
            s += "Notes:\n";
            for n in &self.notes {
                s += &format!("  - {n}\n");
            }
        }
        s
    }
}

/// Channel-keyed store of active scratchpads. Clone the `Arc` freely.
pub struct ScratchpadStore {
    inner: Mutex<HashMap<String, Scratchpad>>,
}

impl ScratchpadStore {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(HashMap::new()),
        })
    }

    /// Render the channel's current scratchpad for the preamble.
    pub fn render(&self, channel: &str) -> String {
        let map = self.inner.lock().unwrap();
        match map.get(channel) {
            Some(p) if !p.is_empty() => p.render(),
            _ => "Scratchpad: empty (no active multi-step task).".to_string(),
        }
    }

    /// A handle bound to one channel — builds the per-channel tools.
    pub fn handle(self: &Arc<Self>, channel: impl Into<String>) -> PadHandle {
        PadHandle {
            store: Arc::clone(self),
            channel: channel.into(),
        }
    }

    fn set_goal(&self, channel: &str, goal: String) -> Value {
        debug!(channel, "scratchpad: set goal");
        let mut map = self.inner.lock().unwrap();
        let pad = map.entry(channel.to_string()).or_default();
        *pad = Scratchpad {
            goal: Some(goal.clone()),
            steps: Vec::new(),
            notes: Vec::new(),
        };
        json!({ "ok": true, "goal": goal })
    }

    fn add_step(&self, channel: &str, text: String) -> Value {
        debug!(channel, "scratchpad: add step");
        let mut map = self.inner.lock().unwrap();
        let pad = map.entry(channel.to_string()).or_default();
        pad.steps.push(Item {
            text,
            status: Status::Pending,
            note: None,
        });
        json!({ "ok": true, "step": pad.steps.len() })
    }

    fn mark(&self, channel: &str, step: usize, status: &str, note: Option<String>) -> Value {
        debug!(channel, step, status, "scratchpad: mark");
        let Some(parsed) = Status::parse(status) else {
            return json!({ "ok": false, "error": "status must be pending|in_progress|done|verified|blocked" });
        };
        let mut map = self.inner.lock().unwrap();
        let Some(pad) = map.get_mut(channel) else {
            return json!({ "ok": false, "error": "no scratchpad; set a goal first" });
        };
        let Some(item) = step.checked_sub(1).and_then(|i| pad.steps.get_mut(i)) else {
            return json!({ "ok": false, "error": format!("no step {step}") });
        };
        item.status = parsed;
        if let Some(n) = note {
            item.note = Some(n);
        }
        json!({ "ok": true, "step": step, "status": parsed.as_str() })
    }

    fn note(&self, channel: &str, text: String) -> Value {
        debug!(channel, "scratchpad: note");
        let mut map = self.inner.lock().unwrap();
        let pad = map.entry(channel.to_string()).or_default();
        pad.notes.push(text);
        json!({ "ok": true })
    }

    fn clear(&self, channel: &str) -> Value {
        debug!(channel, "scratchpad: clear");
        let removed = self.inner.lock().unwrap().remove(channel).is_some();
        json!({ "ok": true, "cleared": removed })
    }
}

/// A scratchpad handle bound to one channel. Builds the rig tools.
#[derive(Clone)]
pub struct PadHandle {
    store: Arc<ScratchpadStore>,
    channel: String,
}

impl PadHandle {
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
/// `store: &ScratchpadStore`, `channel: &str` and the deserialized `args`.
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
                let $channel = self.0.channel.as_str();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goal_step_mark_render_clear() {
        let store = ScratchpadStore::new();
        let ch = "stdin";

        assert!(store.render(ch).contains("empty"));

        assert_eq!(store.set_goal(ch, "ship the thing".into())["ok"], true);
        assert_eq!(store.add_step(ch, "write it".into())["step"], 1);
        assert_eq!(store.add_step(ch, "test it".into())["step"], 2);

        // bad status / out-of-range step are rejected as data, not panics.
        assert_eq!(store.mark(ch, 1, "nonsense", None)["ok"], false);
        assert_eq!(store.mark(ch, 9, "done", None)["ok"], false);

        assert_eq!(store.mark(ch, 1, "verified", None)["ok"], true);
        store.note(ch, "found the bug".into());

        let view = store.render(ch);
        assert!(view.contains("ship the thing"));
        assert!(view.contains("[verified] write it"));
        assert!(view.contains("[pending] test it"));
        assert!(view.contains("found the bug"));

        assert_eq!(store.clear(ch)["cleared"], true);
        assert!(store.render(ch).contains("empty"));
    }
}
