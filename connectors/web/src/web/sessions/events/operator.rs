use octo_history::SessionId;
use tokio::sync::broadcast;

use super::{Entry, EventHub};
use crate::api::models::ProgressItem;

impl EventHub {
    pub fn operator_events(&self) -> broadcast::Receiver<Entry> {
        self.sender.subscribe()
    }
    pub fn operator_steps(&self, id: &SessionId) -> Vec<ProgressItem> {
        let state = self.state.lock().unwrap();
        state
            .resources
            .get(id)
            .and_then(|r| r.operator.current())
            .map(|s| {
                s.items
                    .into_iter()
                    .rev()
                    .take(3)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect()
            })
            .unwrap_or_default()
    }
}
