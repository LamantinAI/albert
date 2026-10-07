//! Host-owned child runs. Connector grants are whole instances, not a sandbox.
pub mod budget;
mod dispatch;
pub mod inspection;
mod tool;
mod workspace;

use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use futures::future::join_all;
use rand::random;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::{sync::watch, time::timeout};

use crate::status::StatusFeed;
pub use dispatch::ScopedDispatch;
use inspection::Settings as InspectionSettings;
pub use tool::{Args, SubagentTool};
pub use workspace::{WorkspaceRead, WorkspaceWrite};

pub type Conversation = (String, String);

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub enabled: bool,
    pub inspection: InspectionSettings,
    pub max_concurrent: usize,
    pub max_retained: usize,
    pub max_runs_per_turn: usize,
    pub max_tool_turns: usize,
    pub default_tool_turns: usize,
    pub timeout_secs: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            enabled: true,
            inspection: Default::default(),
            max_concurrent: 4,
            max_retained: 64,
            max_runs_per_turn: 8,
            max_tool_turns: 17,
            default_tool_turns: 11,
            timeout_secs: 600,
        }
    }
}
impl Limits {
    pub fn validate(&self) -> Result<(), String> {
        if self.max_concurrent == 0
            || self.max_retained < self.max_concurrent
            || self.max_runs_per_turn == 0
            || self.max_tool_turns == 0
            || self.default_tool_turns == 0
            || self.timeout_secs == 0
            || !self.inspection.validate()
        {
            return Err(
                "subagents: limits must be positive; max_retained >= max_concurrent".into(),
            );
        }
        Ok(())
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub task: String,
    #[serde(default)]
    pub context: String,
    pub models: Vec<String>,
    #[serde(default)]
    pub connectors: Vec<String>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub max_tool_turns: Option<usize>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

pub struct Run {
    pub id: String,
    pub parent: String,
    pub conversation: Conversation,
    pub owner: bool,
    pub feed: StatusFeed,
    observed: AtomicBool,
    pub cancel: watch::Sender<bool>,
    pub result: watch::Receiver<Value>,
}
impl Run {
    pub fn acknowledge(&self) {
        if !self.result.borrow().is_null() {
            self.observed.store(true, Ordering::Relaxed);
        }
    }
    fn pending(&self) -> bool {
        !self.observed.load(Ordering::Relaxed)
    }
    pub fn status(&self) -> Value {
        self.result
            .borrow()
            .get("outcome")
            .and_then(|v| v.get("status"))
            .cloned()
            .unwrap_or(json!("running"))
    }

    pub fn summary(&self) -> Value {
        let result = self.result.borrow().clone();
        json!({"run_id":self.id,"parent_run_id":self.parent,"status":self.status(),
            "tool_calls":self.feed.tool_call_count(),"workspace":result.get("workspace"),
            "journal_saved":result.get("journal_saved"),"result_available":!result.is_null(),
            "history_key":format!("subagent/{}",self.id)})
    }

    pub fn view(&self) -> Value {
        json!({"run_id":self.id,"parent_run_id":self.parent,
            "status":self.status(),"result":self.result.borrow().clone(),"tool_calls":self.feed.tool_call_count()})
    }
    pub async fn wait(&self, seconds: u64) -> Value {
        let mut result = self.result.clone();
        let _ = timeout(Duration::from_secs(seconds.min(60)), async {
            while result.borrow().is_null() {
                if result.changed().await.is_err() {
                    break;
                }
            }
        })
        .await;
        self.view()
    }
}

#[derive(Default)]
pub struct Registry(Mutex<State>);
#[derive(Default)]
struct State {
    sequence: u64,
    runs: BTreeMap<u64, Arc<Run>>,
    // Bounded by retained parent runs; the parent's tool instance additionally
    // keeps its own shared counter across model retries.
}
impl Registry {
    pub fn reserve(
        &self,
        limits: &Limits,
        parent: String,
        conversation: Conversation,
        owner: bool,
        prefix: &str,
    ) -> Result<(Arc<Run>, watch::Receiver<bool>, watch::Sender<Value>), String> {
        let mut state = self.0.lock().unwrap();
        if state
            .runs
            .values()
            .filter(|r| r.result.borrow().is_null())
            .count()
            >= limits.max_concurrent
        {
            return Err("Subagent concurrency limit reached; wait for an existing run.".into());
        }
        while state.runs.len() >= limits.max_retained {
            let oldest = state
                .runs
                .iter()
                .find(|(_, r)| !r.result.borrow().is_null() && !r.pending())
                .map(|(id, _)| *id);
            let Some(oldest) = oldest else {
                return Err("Subagent registry is full.".into());
            };
            state.runs.remove(&oldest);
        }
        state.sequence += 1;
        let seq = state.sequence;
        let (cancel, rx) = watch::channel(false);
        let (tx, result) = watch::channel(Value::Null);
        let run = Arc::new(Run {
            id: format!("{prefix}/child-{seq}-{:016x}", random::<u64>()),
            parent,
            conversation,
            owner,
            feed: StatusFeed::silent(),
            observed: AtomicBool::new(false),
            cancel,
            result,
        });
        state.runs.insert(seq, run.clone());
        Ok((run, rx, tx))
    }
    pub fn visible(&self, conversation: &Conversation, owner: bool) -> Vec<Arc<Run>> {
        self.0
            .lock()
            .unwrap()
            .runs
            .values()
            .filter(|r| &r.conversation == conversation && (!r.owner || owner))
            .cloned()
            .collect()
    }
    pub fn get(
        &self,
        id: &str,
        conversation: &Conversation,
        owner: bool,
    ) -> Result<Arc<Run>, String> {
        self.visible(conversation, owner)
            .into_iter()
            .find(|r| r.id == id)
            .ok_or_else(|| "Unknown or inaccessible subagent run.".into())
    }
    pub fn context(&self, conversation: &Conversation, owner: bool) -> String {
        let runs: Vec<_> = self
            .visible(conversation, owner)
            .iter()
            .filter(|r| r.pending())
            .map(|r| json!({"run_id":r.id,"status":r.status()}))
            .collect();
        if runs.is_empty() {
            return String::new();
        }
        format!(
            "\n\nDelegated runs awaiting collection (subagent inspect/wait):\n{}",
            json!(runs)
        )
    }

    pub async fn cancel_all(&self) {
        let conversations: Vec<_> = self
            .0
            .lock()
            .unwrap()
            .runs
            .values()
            .map(|r| r.conversation.clone())
            .collect();
        for conversation in conversations {
            self.cancel_channel(&conversation).await;
        }
    }
    pub async fn cancel_channel(&self, conversation: &Conversation) -> bool {
        let runs = self.visible(conversation, true);
        let mut cancelled = false;
        for run in &runs {
            if run.result.borrow().is_null() {
                cancelled = true;
                let _ = run.cancel.send(true);
            }
        }
        join_all(runs.iter().map(|run| run.wait(60))).await;
        cancelled
    }
}

#[cfg(test)]
mod tests {
    use super::workspace::{ReadArgs, WriteArgs};
    use super::*;
    use octo_core::{ConnectorId, EventBus, EventKind, Filter, InProcessBus, SubscribeOptions};
    use octo_rig::{DispatchArgs, OctoDispatchTool};
    use rig::tool::Tool;
    use serde_json::{json, Value};
    use std::{collections::HashMap, sync::Arc, time::Duration};
    use tempfile::tempdir;
    use tokio::time::timeout;

    #[tokio::test]
    async fn denied_targets_and_runtime_control_never_reach_bus() {
        let bus = Arc::new(InProcessBus::new(16));
        let mut events = bus
            .subscribe(Filter::all(), SubscribeOptions::default())
            .await
            .unwrap();
        let tool = ScopedDispatch {
            inner: OctoDispatchTool::new(bus, ConnectorId::new("child"), "allowed"),
            allowed: HashMap::from([("allowed".into(), vec![EventKind::new("allowed.**")])]),
        };
        for (target, kind) in [
            ("hidden", "secret.read"),
            ("allowed", "octo.control.restart_process"),
            ("allowed", "octo.control.cancel"),
            ("allowed", "chat.message"),
            ("allowed", "alarm.fired"),
        ] {
            let result = tool
                .call(DispatchArgs {
                    target: target.into(),
                    kind: kind.into(),
                    channel: None,
                    payload: Value::Null,
                })
                .await
                .unwrap();
            assert_eq!(result["status"], "not_sent");
        }
        assert!(timeout(Duration::from_millis(30), events.next())
            .await
            .is_err());
    }

    #[tokio::test]
    async fn registry_bounds_concurrency_and_partitions_conversations_and_owner_work() {
        let registry = Registry::default();
        let limits = Limits {
            max_concurrent: 1,
            max_retained: 1,
            ..Default::default()
        };
        let room = ("telegram".into(), "42".into());
        let (first, _, done) = registry
            .reserve(&limits, "parent".into(), room.clone(), true, "test")
            .unwrap();
        assert!(registry
            .reserve(&limits, "parent".into(), room.clone(), false, "test")
            .is_err());
        assert!(registry.get(&first.id, &room, false).is_err());
        assert!(registry
            .get(&first.id, &("other".into(), "42".into()), true)
            .is_err());
        done.send_replace(json!({"outcome":{"status":"completed"}}));
        first.acknowledge();
        let (second, _, _) = registry
            .reserve(&limits, "parent".into(), room.clone(), false, "test")
            .unwrap();
        assert_ne!(first.id, second.id);
        assert!(registry.get(&first.id, &room, true).is_err());
        assert!(registry.get(&second.id, &room, false).is_ok());
    }

    #[tokio::test]
    async fn workspaces_are_independent_and_traversal_cannot_reach_sibling() {
        let root = tempdir().unwrap();
        let first = root.path().join("first");
        let second = root.path().join("second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        WorkspaceWrite(first.clone())
            .call(WriteArgs {
                path: "note.txt".into(),
                content: "first only".into(),
            })
            .await
            .unwrap();
        let own = WorkspaceRead(first)
            .call(ReadArgs {
                path: "note.txt".into(),
            })
            .await
            .unwrap();
        assert_eq!(own["content"], "first only");
        for path in ["note.txt", "../first/note.txt"] {
            let denied = WorkspaceRead(second.clone())
                .call(ReadArgs { path: path.into() })
                .await
                .unwrap();
            assert!(denied.get("error").is_some());
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.path().join("first"), second.join("escape")).unwrap();
            let denied = WorkspaceRead(second)
                .call(ReadArgs {
                    path: "escape/note.txt".into(),
                })
                .await
                .unwrap();
            assert!(denied.get("error").is_some());
        }
    }
}
