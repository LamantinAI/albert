use std::sync::{Arc, Mutex};

use octo_history::AppendReceipt;
use tokio::sync::{
    oneshot,
    oneshot::{Receiver, Sender},
};

type Outcome = Result<AppendReceipt, String>;

/// Transient native acknowledgement; SQL/RAM journal receipts remain the source
/// of truth after disconnect. This token never appears in a serialized envelope.
#[derive(Clone)]
pub(crate) struct SourceReceipt(Arc<Mutex<Option<Sender<Outcome>>>>);

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
