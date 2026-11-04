use std::{sync::atomic::AtomicBool, sync::atomic::Ordering, sync::Arc, sync::Mutex};

use octo_history::AppendReceipt;
use tokio::{sync::oneshot, sync::oneshot::Receiver, sync::oneshot::Sender};

type Outcome = Result<AppendReceipt, String>;

/// Transient native acknowledgement; SQL/RAM journal receipts remain the source
/// of truth after disconnect. This token never appears in a serialized envelope.
#[derive(Clone)]
pub struct SourceReceipt(Arc<Mutex<Option<Sender<Outcome>>>>);

/// Non-serializable, single-dispatch proof for input already atomically journaled
/// by the web first-run admission path. A replay never manufactures this proof.
#[derive(Clone)]
pub struct AcceptedStart {
    pub session: String,
    pub request: String,
    pub digest: String,
    used: Arc<AtomicBool>,
}
impl AcceptedStart {
    pub fn new(session: String, request: String, digest: String) -> Self {
        Self {
            session,
            request,
            digest,
            used: Arc::new(AtomicBool::new(false)),
        }
    }
    pub fn consume(&self, session: &str, request: &str, digest: &str) -> bool {
        self.session == session
            && self.request == request
            && self.digest == digest
            && self
                .used
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
    }
}

impl SourceReceipt {
    pub fn channel() -> (Self, Receiver<Outcome>) {
        let (sender, receiver) = oneshot::channel();
        (Self(Arc::new(Mutex::new(Some(sender)))), receiver)
    }
    pub fn complete(&self, outcome: Outcome) {
        if let Some(sender) = self.0.lock().unwrap().take() {
            let _ = sender.send(outcome);
        }
    }
}
