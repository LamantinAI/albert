//! One child run, observed result and cancellation lifecycle.
use super::Conversation;
use crate::{sessions::SessionContext, status::StatusFeed};
use serde_json::{json, Value};
use std::{sync::atomic::AtomicBool, sync::atomic::Ordering, sync::Mutex, time::Duration};
use tokio::{sync::watch, time::timeout};
pub struct Run {
    pub id: String,
    pub parent: String,
    pub conversation: Conversation,
    pub owner: bool,
    pub feed: StatusFeed,
    pub(crate) session: Option<SessionContext>,
    pub(super) observed: AtomicBool,
    pub cancel: watch::Sender<bool>,
    pub result: watch::Receiver<Value>,
    pub(super) completion: watch::Sender<Value>,
    pub(super) disposed: Mutex<bool>,
}
impl Run {
    pub(crate) fn is_live(&self) -> bool {
        !*self.disposed.lock().unwrap() && self.session.as_ref().is_none_or(SessionContext::is_live)
    }
    pub(crate) fn complete(&self, value: Value) {
        let disposed = self.disposed.lock().unwrap();
        if !*disposed && self.session.as_ref().is_none_or(SessionContext::is_live) {
            self.completion.send_replace(value);
        } else {
            self.completion
                .send_replace(json!({"outcome":{"status":"closed"}}));
        }
    }
    pub(super) fn discard(&self) -> Result<(), &'static str> {
        let mut poisoned = false;
        *self.disposed.lock().unwrap_or_else(|error| {
            poisoned = true;
            error.into_inner()
        }) = true;
        let feed = self.feed.discard();
        self.completion
            .send_replace(json!({"outcome":{"status":"closed"}}));
        if poisoned {
            Err("child state was poisoned during cleanup")
        } else {
            feed
        }
    }
    pub(crate) fn history_key(&self) -> String {
        if self.conversation.0 == "session" {
            format!("{}/child/{}", self.conversation.1, self.id)
        } else {
            format!("subagent/{}", self.id)
        }
    }
    pub fn acknowledge(&self) {
        if !self.result.borrow().is_null() {
            self.observed.store(true, Ordering::Relaxed);
        }
    }
    pub(super) fn pending(&self) -> bool {
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

    pub(crate) fn inspection_summary(&self) -> Value {
        let result = self.result.borrow();
        let outcome = result.get("outcome");
        let error = outcome
            .and_then(|outcome| outcome.get("error"))
            .and_then(Value::as_str)
            .map(|text| text.chars().take(96).collect::<String>());
        json!({"run_id":self.id,"status":self.status(),"tool_calls":self.feed.tool_call_count(),"result_available":!result.is_null(),"journal_saved":result.get("journal_saved"),
            "result_metadata":{"error":error,"tool_rounds":outcome.and_then(|value|value.get("tool_rounds")),"tool_round_limit":outcome.and_then(|value|value.get("tool_round_limit")),"budget_exhausted":outcome.and_then(|value|value.get("budget_exhausted"))},
            "result_ref":{"action":"wait","run_id":self.id,"seconds":0}})
    }
    pub fn summary(&self) -> Value {
        let result = self.result.borrow().clone();
        json!({"run_id":self.id,"parent_run_id":self.parent,"status":self.status(),
            "tool_calls":self.feed.tool_call_count(),"workspace":result.get("workspace"),
            "journal_saved":result.get("journal_saved"),"result_available":!result.is_null(),
            "history_key":self.history_key()})
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
