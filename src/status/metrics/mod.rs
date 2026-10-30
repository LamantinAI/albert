//! Content-free host diagnostics. A tool retains its originating request scope
//! even if a later round has updated the feed before its completion is observed.
use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};

use tracing::info;

use super::StatusFeed;
use crate::context::DiagnosticScope;

#[derive(Default)]
pub(super) struct ToolMetrics {
    scope: Mutex<Option<DiagnosticScope>>,
    request_id: Mutex<Option<String>>,
    names: Mutex<HashSet<String>>,
    started: Mutex<HashMap<String, Started>>,
    sequence: AtomicU64,
}
struct Started {
    scope: Option<DiagnosticScope>,
    request_id: Option<String>,
    tool: String,
    sequence: u64,
    at: Instant,
}
struct Observation {
    scope: Option<DiagnosticScope>,
    request_id: Option<String>,
    tool: String,
    sequence: u64,
    elapsed: Duration,
    output_bytes: usize,
}
impl StatusFeed {
    pub(crate) fn with_diagnostics(self, scope: Option<DiagnosticScope>) -> Self {
        self.set_diagnostics(scope);
        self
    }
    pub(crate) fn set_diagnostics(&self, scope: Option<DiagnosticScope>) {
        self.metrics.set_scope(scope);
    }
    pub(crate) fn diagnostics(&self) -> Option<DiagnosticScope> {
        self.metrics.scope()
    }
    pub(crate) fn diagnostics_for(&self, compact: bool) -> Option<DiagnosticScope> {
        self.diagnostics().map(|mut scope| {
            if compact {
                scope.lane = "compact";
            }
            scope
        })
    }
    pub(crate) fn set_request_id(&self, id: &str) {
        self.metrics.set_request_id(id);
    }
    pub(crate) fn request_id(&self) -> Option<String> {
        self.metrics.request_id()
    }
    pub(crate) fn register_tool_names(&self, names: impl IntoIterator<Item = String>) {
        self.metrics.register(names);
    }

    pub(super) fn start_tool_metric(&self, id: &str, name: &str, external: bool) {
        self.metrics.start(id, name, external, self.request_id());
    }
}

impl ToolMetrics {
    pub fn scope(&self) -> Option<DiagnosticScope> {
        self.scope.lock().unwrap().clone()
    }
    pub fn set_scope(&self, scope: Option<DiagnosticScope>) {
        *self.scope.lock().unwrap() = scope;
    }
    pub fn request_id(&self) -> Option<String> {
        self.request_id.lock().unwrap().clone()
    }
    pub fn set_request_id(&self, id: &str) {
        *self.request_id.lock().unwrap() = Some(id.into());
    }
    pub fn register(&self, names: impl IntoIterator<Item = String>) {
        *self.names.lock().unwrap() = names.into_iter().collect();
    }
    pub fn clear(&self) {
        self.started.lock().unwrap().clear();
    }
    pub fn start(&self, id: &str, name: &str, host_external: bool, request_id: Option<String>) {
        let trusted = host_external || self.names.lock().unwrap().contains(name);
        self.started.lock().unwrap().insert(
            id.into(),
            Started {
                scope: self.scope(),
                request_id,
                tool: if trusted {
                    name.into()
                } else {
                    "unregistered_tool".into()
                },
                sequence: self.sequence.fetch_add(1, Ordering::Relaxed),
                at: Instant::now(),
            },
        );
    }
    fn complete(&self, id: &str, output_bytes: usize) -> Option<Observation> {
        let started = self.started.lock().unwrap().remove(id)?;
        Some(Observation {
            scope: started.scope,
            request_id: started.request_id,
            tool: started.tool,
            sequence: started.sequence,
            elapsed: started.at.elapsed(),
            output_bytes,
        })
    }
    pub fn finish(&self, id: &str, output_bytes: usize) {
        let Some(observed) = self.complete(id, output_bytes) else {
            return;
        };
        let scope = observed.scope.as_ref();
        info!(
            lane = scope.map(|scope| scope.lane).unwrap_or("unspecified"),
            session_id = scope.and_then(|scope| scope.session_id.as_deref()),
            work_id = scope.and_then(|scope| scope.work_id.as_deref()),
            attempt_id = scope.map(|scope| scope.attempt_id.as_str()),
            run_id = scope.and_then(|scope| scope.run_id.as_deref()),
            request_id = observed.request_id.as_deref(),
            tool = observed.tool,
            tool_sequence = observed.sequence,
            tool_elapsed_ms = observed.elapsed.as_millis() as u64,
            model_visible_output_bytes = observed.output_bytes,
            "tool completed"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::ToolMetrics;
    use crate::context::DiagnosticScope;
    fn scope(lane: &'static str, attempt: &str) -> DiagnosticScope {
        DiagnosticScope {
            lane,
            session_id: Some("host-session".into()),
            work_id: Some("host-work".into()),
            attempt_id: attempt.into(),
            run_id: (lane == "child").then(|| "host-run".into()),
        }
    }
    #[test]
    fn concurrent_tools_keep_starting_request_and_scope_without_any_content() {
        let metrics = ToolMetrics::default();
        metrics.register(["safe_tool".into()]);
        metrics.set_scope(Some(scope("native", "first-attempt")));
        metrics.set_request_id("request-one");
        metrics.start("internal-a", "safe_tool", false, metrics.request_id());
        metrics.set_scope(Some(scope("child", "child-attempt")));
        metrics.set_request_id("request-two");
        metrics.start("internal-b", "safe_tool", false, metrics.request_id());
        let first = metrics.complete("internal-a", 123).unwrap();
        let second = metrics.complete("internal-b", 456).unwrap();
        assert_eq!(first.scope.unwrap().attempt_id, "first-attempt");
        assert_eq!(first.request_id.as_deref(), Some("request-one"));
        assert_eq!(first.output_bytes, 123);
        assert_eq!(second.scope.unwrap().run_id.as_deref(), Some("host-run"));
        assert_eq!(second.request_id.as_deref(), Some("request-two"));
        assert_eq!(second.output_bytes, 456);
        assert_ne!(first.sequence, second.sequence);
        assert!(metrics.complete("internal-a", 0).is_none());
    }
    #[test]
    fn unregistered_model_name_is_not_logged_as_trusted_metadata() {
        let metrics = ToolMetrics::default();
        metrics.start(
            "internal",
            "secret-model-supplied-name",
            false,
            metrics.request_id(),
        );
        assert_eq!(
            metrics.complete("internal", 0).unwrap().tool,
            "unregistered_tool"
        );
        metrics.start(
            "host-external",
            "dispatch_to_connector",
            true,
            metrics.request_id(),
        );
        assert_eq!(
            metrics.complete("host-external", 1).unwrap().tool,
            "dispatch_to_connector"
        );
    }
}
