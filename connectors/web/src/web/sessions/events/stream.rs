use crate::web::sessions::SessionHost;
use crate::{
    api::errors::ApiError, api::errors::ApiResult, api::host::EventStream,
    api::host::SessionEventRequest, api::principal::Principal,
};
use futures::stream::unfold;
use serde_json::{json, to_value};
use std::{collections::VecDeque, future::pending};
use tokio::{select, sync::broadcast::error::RecvError};
impl SessionHost {
    pub(in crate::web::sessions) async fn event_stream(
        &self,
        caller: Principal,
        request: SessionEventRequest,
    ) -> ApiResult<EventStream> {
        let actor = self.actor(&caller).await?;
        let context = self.resource(&actor, &request.session_id).await?;
        let receiver = self.hub.sender.subscribe();
        let credential_events = self.runtime.sessions.credentials().subscribe();
        let approval_events = self
            .approvals
            .read()
            .unwrap()
            .as_ref()
            .map(|service| service.subscribe());
        let account_id = actor.account_id.clone();
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
                queue,
                receiver,
                context,
                shutdown,
                hub,
                id,
                owner,
                epoch,
                0u64,
                credential_events,
                approval_events,
                account_id,
            ),
            |(
                mut queue,
                mut receiver,
                context,
                shutdown,
                hub,
                id,
                owner,
                epoch,
                mut last,
                mut credential_events,
                mut approval_events,
                account_id,
            )| async move {
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
                                queue,
                                receiver,
                                context,
                                shutdown,
                                hub,
                                id,
                                owner,
                                epoch,
                                last,
                                credential_events,
                                approval_events,
                                account_id,
                            ),
                        ));
                    }
                    select! {
                        _=shutdown.cancelled()=>return None,
                        credential=credential_events.recv()=>match credential {
                            Ok(change) if change.account_id==account_id && change.session_id.as_deref()==Some(id.as_str())=>queue.push_back(hub.record(&id,&owner,&epoch,"credentials",None,json!({"request_id":change.request_id}))),
                            Err(RecvError::Lagged(_))=>queue.push_back(hub.record(&id,&owner,&epoch,"credentials",None,json!({"resync":true}))),
                            Err(RecvError::Closed)=>return None,
                            _=>{},
                        },
                        approval=async {match &mut approval_events {Some(receiver)=>receiver.recv().await,None=>pending().await}}=>match approval {
                            Ok(change) if change.account.is_empty() && change.id.is_empty()=>queue.push_back(hub.record(&id,&owner,&epoch,"approvals",None,json!({"resync":true}))),
                            Ok(change) if change.account==account_id && change.session.as_deref()==Some(id.as_str())=>queue.push_back(hub.record(&id,&owner,&epoch,"approvals",None,json!({"request_id":change.id}))),
                            Err(RecvError::Lagged(_))=>queue.push_back(hub.record(&id,&owner,&epoch,"approvals",None,json!({"resync":true}))),
                            Err(RecvError::Closed)=>approval_events=None,
                            _=>{},
                        },
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
