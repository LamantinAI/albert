mod operator;
mod stream;
use std::{collections::HashMap, collections::VecDeque, sync::Arc, sync::Mutex};

use albert_core::{
    sessions::transport::OutputScope, sessions::transport::TurnFeedback, sessions::SessionContext,
    status::progress::ProgressUpdate, status::text::TextUpdate,
};
use octo_core::Envelope;
use octo_history::{SessionAccess, SessionId};
use serde_json::{from_value, json, to_value, Value};
use tokio::sync::broadcast;

use super::{progress::Progress, text::TextPreview, SessionHost};
use crate::{
    api::models::ProgressDelta, api::models::ProgressSnapshot, api::models::SessionEvent,
    api::models::TextDelta, api::models::TextSnapshot,
};

#[derive(Clone)]
pub(super) struct Entry {
    pub(super) owner: String,
    sequence: u64,
    pub(super) event: SessionEvent,
}

#[derive(Default)]
struct Resource {
    entries: VecDeque<Entry>,
    attempt: Option<String>,
    status: Vec<String>,
    progress: Progress,
    operator: Progress,
    text: TextPreview,
}
#[derive(Default)]
struct State {
    sequence: u64,
    resources: HashMap<SessionId, Resource>,
    order: VecDeque<SessionId>,
}
pub(super) struct EventHub {
    state: Mutex<State>,
    sender: broadcast::Sender<Entry>,
}
impl EventHub {
    pub fn new() -> Arc<Self> {
        let (sender, _) = broadcast::channel(128);
        Arc::new(Self {
            state: Mutex::new(State::default()),
            sender,
        })
    }
    fn record(
        &self,
        id: &SessionId,
        owner: &str,
        epoch: &str,
        kind: &str,
        attempt: Option<String>,
        data: Value,
    ) -> Entry {
        let mut state = self.state.lock().unwrap();
        if !state.resources.contains_key(id) {
            if state.resources.len() >= 256 {
                if let Some(old) = state.order.pop_front() {
                    state.resources.remove(&old);
                }
            }
            state.order.push_back(id.clone());
            state.resources.insert(id.clone(), Resource::default());
        }
        state.sequence += 1;
        let sequence = state.sequence;
        let event = SessionEvent {
            id: format!("{epoch}/{id}/{sequence}"),
            session_id: id.to_string(),
            runtime_epoch: epoch.into(),
            kind: kind.into(),
            attempt_id: attempt.clone(),
            data: data.clone(),
        };
        let entry = Entry {
            owner: owner.into(),
            sequence,
            event,
        };
        let resource = state.resources.get_mut(id).expect("inserted resource");
        if kind == "operator_progress" {
            if let Ok(delta) = from_value::<ProgressDelta>(data.clone()) {
                resource.operator.apply(&delta);
            }
        }
        if kind == "text" {
            let Ok(delta) = from_value::<TextDelta>(data.clone()) else {
                return entry;
            };
            if !resource.text.apply(&delta) {
                return entry;
            }
        }
        if kind == "progress" {
            let Ok(delta) = from_value::<ProgressDelta>(data) else {
                return entry;
            };
            if !resource.progress.apply(&delta) {
                return entry;
            }
        }
        match kind {
            "typing" => {
                resource.attempt = attempt;
                resource.status = vec!["Working…".into()];
            }
            "status" => {
                if resource.attempt == attempt {
                    resource.status = vec!["Tool activity in progress".into()];
                }
            }
            "attempt_ended" if resource.attempt == attempt => {
                resource.status.clear();
                resource.attempt = None;
            }
            _ => {}
        }
        resource.entries.push_back(entry.clone());
        if resource.entries.len() > 128 {
            resource.entries.pop_front();
        }
        drop(state);
        let _ = self.sender.send(entry.clone());
        entry
    }
    pub fn changed(
        &self,
        id: &SessionId,
        owner: &str,
        epoch: &str,
        kind: &str,
        attempt: Option<String>,
    ) {
        self.record(id, owner, epoch, kind, attempt, json!({"reason":kind}));
    }
    pub fn progress(&self, id: &SessionId, active: Option<&str>) -> Option<ProgressSnapshot> {
        self.state
            .lock()
            .unwrap()
            .resources
            .get(id)?
            .progress
            .snapshot(active)
    }
    pub fn text(&self, id: &SessionId, active: Option<&str>) -> Option<TextSnapshot> {
        self.state
            .lock()
            .unwrap()
            .resources
            .get(id)?
            .text
            .snapshot(active)
    }
    pub fn status(&self, id: &SessionId) -> Vec<String> {
        self.state
            .lock()
            .unwrap()
            .resources
            .get(id)
            .map(|resource| resource.status.clone())
            .unwrap_or_default()
    }
    pub fn remove(&self, id: &SessionId) {
        let mut state = self.state.lock().unwrap();
        state.resources.remove(id);
        state.order.retain(|existing| existing != id);
    }
    pub fn observe(&self, envelope: &Envelope, epoch: &str) {
        let Some(context) = envelope.context.get::<SessionContext>() else {
            return;
        };
        let SessionAccess::Private { owner } = &context.session().access else {
            return;
        };
        if !context.is_private() || !context.is_live() {
            return;
        }
        if envelope.kind.as_str() == "albert.operator.progress" {
            if let Some(update) = envelope.context.get::<ProgressUpdate>() {
                if let Ok(data) = to_value(ProgressDelta::from(update.0.clone())) {
                    self.record(
                        context.session_id(),
                        owner,
                        epoch,
                        "operator_progress",
                        Some(update.0.attempt_id.clone()),
                        data,
                    );
                }
            }
            return;
        }
        if envelope.kind.as_str() == "albert.text" {
            if let Some(update) = envelope.context.get::<TextUpdate>() {
                if let Ok(data) = to_value(TextDelta::from(update.0.clone())) {
                    self.record(
                        context.session_id(),
                        owner,
                        epoch,
                        "text",
                        Some(update.0.attempt_id.clone()),
                        data,
                    );
                }
            }
            return;
        }
        if envelope.kind.as_str() == "albert.progress" {
            if envelope
                .context
                .get::<TurnFeedback>()
                .is_some_and(|token| !token.is_open())
            {
                return;
            }
            if let Some(update) = envelope.context.get::<ProgressUpdate>() {
                if let Ok(data) = to_value(ProgressDelta::from(update.0.clone())) {
                    self.record(
                        context.session_id(),
                        owner,
                        epoch,
                        "progress",
                        Some(update.0.attempt_id.clone()),
                        data,
                    );
                }
            }
            return;
        }
        let kind = match envelope.kind.as_str() {
            "albert.session.updated" => "session",
            "chat.typing" => "typing",
            "chat.status" => "status",
            "chat.reply" => "changed",
            "chat.turn.end" => "attempt_ended",
            _ => return,
        };
        if matches!(kind, "typing" | "status")
            && envelope
                .context
                .get::<TurnFeedback>()
                .is_some_and(|token| !token.is_open())
        {
            return;
        }
        let attempt = envelope
            .context
            .get::<TurnFeedback>()
            .map(|token| token.id().to_owned())
            .or_else(|| {
                envelope
                    .context
                    .get::<OutputScope>()
                    .and_then(|scope| scope.attempt_id.clone())
            });
        self.changed(context.session_id(), owner, epoch, kind, attempt);
    }
    fn replay(&self, id: &SessionId, owner: &str, sequence: u64) -> Option<VecDeque<Entry>> {
        let state = self.state.lock().unwrap();
        let resource = state.resources.get(id)?;
        if !resource
            .entries
            .iter()
            .any(|entry| entry.sequence == sequence && entry.owner == owner)
        {
            return None;
        }
        Some(
            resource
                .entries
                .iter()
                .filter(|entry| entry.sequence > sequence && entry.owner == owner)
                .cloned()
                .collect(),
        )
    }
}
impl SessionHost {}

#[cfg(test)]
mod tests {
    use octo_history::SessionId;
    use serde_json::json;

    use super::EventHub;

    #[test]
    fn snapshot_payload_is_identical_in_initial_broadcast_and_replay() {
        let hub = EventHub::new();
        let id = SessionId::parse("01900000-0000-7000-8000-000000000001").unwrap();
        let anchor = hub.record(
            &id,
            "owner",
            "epoch",
            "changed",
            None,
            json!({"reason":"changed"}),
        );
        let mut receiver = hub.sender.subscribe();
        let snapshot = hub.record(
            &id,
            "owner",
            "epoch",
            "snapshot",
            None,
            json!({"state":{"status":["Working"]}}),
        );
        let broadcast = receiver.try_recv().unwrap();
        let replay = hub.replay(&id, "owner", anchor.sequence).unwrap();
        assert_eq!(snapshot.event.data, broadcast.event.data);
        assert_eq!(snapshot.event.data, replay[0].event.data);
        assert!(hub
            .replay(&id, "different-owner", anchor.sequence)
            .is_none());
    }
}
