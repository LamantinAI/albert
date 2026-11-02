use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};

use futures::stream::unfold;
use octo_connector_telegram::{TelegramOutputScope, TurnFeedback};
use octo_core::Envelope;
use octo_history::{SessionAccess, SessionId};
use serde_json::{from_value, json, to_value, Value};
use tokio::{select, sync::broadcast, sync::broadcast::error::RecvError};

use super::{progress::Progress, text::TextPreview, SessionHost};
use crate::{
    api::{
        errors::{ApiError, ApiResult},
        host::{EventStream, SessionEventRequest},
        models::{ProgressDelta, ProgressSnapshot, SessionEvent, TextDelta, TextSnapshot},
        principal::Principal,
    },
    sessions::SessionContext,
    status::{progress::ProgressUpdate, text::TextUpdate},
};

#[derive(Clone)]
struct Entry {
    owner: String,
    sequence: u64,
    event: SessionEvent,
}

#[derive(Default)]
struct Resource {
    entries: VecDeque<Entry>,
    attempt: Option<String>,
    status: Vec<String>,
    progress: Progress,
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
        if envelope.kind.as_str() == "albert.text" {
            if let Some(update) = envelope.context.get::<TextUpdate>() {
                if let Ok(data) = to_value(&update.0) {
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
                if let Ok(data) = to_value(&update.0) {
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
                    .get::<TelegramOutputScope>()
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
impl SessionHost {
    pub(super) async fn event_stream(
        &self,
        caller: Principal,
        request: SessionEventRequest,
    ) -> ApiResult<EventStream> {
        let actor = self.actor(&caller).await?;
        let context = self.resource(&actor, &request.session_id).await?;
        let receiver = self.hub.sender.subscribe();
        let epoch = self.runtime.status().runtime_epoch;
        let id = context.session_id().clone();
        let owner = actor.user_id.as_str().to_owned();
        let mut queue = VecDeque::new();
        let mut resync = request
            .runtime_epoch
            .as_ref()
            .is_some_and(|requested| requested != &epoch);
        if let Some(after) = request.after {
            let parts: Vec<_> = after.split('/').collect();
            if parts.len() != 3 || parts[1] != request.session_id {
                return Err(ApiError::invalid("Event cursor belongs to another chat."));
            }
            if parts[0] != epoch {
                resync = true;
            } else {
                let sequence = parts[2]
                    .parse::<u64>()
                    .map_err(|_| ApiError::invalid("Invalid event cursor."))?;
                match self.hub.replay(&id, &owner, sequence) {
                    Some(events) => queue = events,
                    None => resync = true,
                }
            }
        }
        let snapshot = self.state(&actor, &request.session_id).await?;
        let initial = self.hub.record(
            &id,
            &owner,
            &epoch,
            if resync { "resync" } else { "snapshot" },
            None,
            json!({"state":to_value(snapshot).map_err(|_|ApiError::internal())?,"resync":resync}),
        );
        queue.push_back(initial);
        let shutdown = self
            .context
            .read()
            .unwrap()
            .as_ref()
            .map(|context| context.shutdown.clone())
            .unwrap_or_default();
        let hub = self.hub.clone();
        let stream = unfold(
            (
                queue, receiver, context, shutdown, hub, id, owner, epoch, 0u64,
            ),
            |(mut queue, mut receiver, context, shutdown, hub, id, owner, epoch, mut last)| async move {
                loop {
                    if !context.is_live() {
                        return None;
                    }
                    if let Some(entry) = queue.pop_front() {
                        if entry.sequence <= last {
                            continue;
                        }
                        last = entry.sequence;
                        return Some((
                            Ok(entry.event),
                            (
                                queue, receiver, context, shutdown, hub, id, owner, epoch, last,
                            ),
                        ));
                    }
                    select! {
                        _=shutdown.cancelled()=>return None,
                        event=receiver.recv()=>match event {
                            Ok(entry) if entry.owner==owner && entry.event.session_id==id.as_str()=>queue.push_back(entry),
                            Ok(_)=>{},
                            Err(RecvError::Lagged(_))=>queue.push_back(hub.record(&id,&owner,&epoch,"resync",None,json!({"reason":"resync"}))),
                            Err(RecvError::Closed)=>return None,
                        }
                    }
                }
            },
        );
        Ok(Box::pin(stream))
    }
}

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
