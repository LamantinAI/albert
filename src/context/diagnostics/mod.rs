//! Host-provided correlation only. No prompt, tool arguments, provider payload
//! or serialized caller authority belongs in these identities.
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use rand::random;
use tracing::{info_span, Span};

#[derive(Clone, Debug)]
pub(crate) struct DiagnosticScope {
    pub lane: &'static str,
    pub session_id: Option<String>,
    pub work_id: Option<String>,
    pub attempt_id: String,
    pub run_id: Option<String>,
}

#[derive(Clone)]
pub(super) struct Diagnostics {
    pub scope: Option<DiagnosticScope>,
    pub model: Option<String>,
    nonce: u64,
    rounds: Arc<AtomicU64>,
}

impl Default for Diagnostics {
    fn default() -> Self {
        Self {
            scope: None,
            model: None,
            nonce: random(),
            rounds: Arc::new(AtomicU64::new(0)),
        }
    }
}

impl Diagnostics {
    pub fn next(&self, mode: &'static str) -> DiagnosticRound {
        let round = self.rounds.fetch_add(1, Ordering::Relaxed) + 1;
        let scope = self.scope.as_ref();
        let request = format!("{:016x}-{round}", self.nonce);
        let span = info_span!("model_request",
            request_id = %request, round, mode,
            lane = scope.map(|s| s.lane).unwrap_or("unscoped"),
            session_id = scope.and_then(|s| s.session_id.as_deref()).unwrap_or(""),
            work_id = scope.and_then(|s| s.work_id.as_deref()).unwrap_or(""),
            attempt_id = scope.map(|s| s.attempt_id.as_str()).unwrap_or(""),
            run_id = scope.and_then(|s| s.run_id.as_deref()).unwrap_or(""),
            model_id = self.model.as_deref().unwrap_or(""));
        DiagnosticRound { id: request, span }
    }
}

pub(super) struct DiagnosticRound {
    pub id: String,
    pub span: Span,
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use super::Diagnostics;

    #[test]
    fn cloned_wrappers_share_a_counter_but_new_wrappers_have_distinct_namespaces() {
        let first = Diagnostics::default();
        let clone = first.clone();
        first.next("completion");
        clone.next("completion");
        assert_eq!(first.rounds.load(Ordering::Relaxed), 2);
        let other = Diagnostics::default();
        assert_ne!(first.nonce, other.nonce);
        assert_eq!(other.rounds.load(Ordering::Relaxed), 0);
    }
}
