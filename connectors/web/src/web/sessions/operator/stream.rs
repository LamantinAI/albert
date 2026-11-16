use std::{collections::HashSet, future::pending, sync::Arc, time::Duration};

use futures::stream::unfold;
use serde_json::to_value;
use tokio::{
    select, sync::broadcast::error::RecvError, time::interval_at, time::sleep, time::Instant,
};

use super::super::SessionHost;
use crate::{
    api::errors::ApiResult, api::host::EventStream, api::models::OperatorQuery,
    api::models::SessionEvent, api::principal::Principal,
};

impl SessionHost {
    pub async fn operator_stream(
        self: &Arc<Self>,
        caller: Principal,
        query: OperatorQuery,
    ) -> ApiResult<EventStream> {
        let (actor, contexts) = self.operator_contexts(&caller, &query).await?;
        let events = self.hub.operator_events();
        let credentials = self.runtime.sessions.credentials().subscribe();
        let approvals = self
            .approvals
            .read()
            .unwrap()
            .as_ref()
            .map(|service| service.subscribe());
        let ids: HashSet<_> = contexts
            .iter()
            .map(|c| c.session_id().to_string())
            .collect();
        let attention = self.operator_attention(&caller, &actor, &contexts).await;
        let state = (
            self.clone(),
            caller,
            actor,
            contexts,
            ids,
            events,
            credentials,
            approvals,
            attention,
            interval_at(
                Instant::now() + Duration::from_secs(10),
                Duration::from_secs(10),
            ),
            0u64,
        );
        Ok(Box::pin(unfold(
            state,
            |(
                host,
                caller,
                actor,
                contexts,
                ids,
                mut events,
                mut credentials,
                mut approvals,
                mut attention,
                mut tick,
                mut sequence,
            )| async move {
                let mut attention_changed = false;
                let mut catalog_changed = false;
                if sequence > 0 {
                    loop {
                        select! {
                            _=tick.tick()=>{attention_changed=true;break;},
                            event=events.recv()=>match event {
                                Ok(event) if event.owner==actor.user_id.as_str() && (ids.contains(&event.event.session_id) || matches!(event.event.kind.as_str(),"session"|"operator_progress"))=>{catalog_changed= !ids.contains(&event.event.session_id) || matches!(event.event.kind.as_str(),"session"|"changed"|"ended");break;},
                                Err(RecvError::Lagged(_))=>{attention_changed=true;break;},
                                Err(RecvError::Closed)=>return None,
                                _=>{},
                            },
                            event=credentials.recv()=>match event {
                                Ok(event) if event.account_id==actor.account_id && event.session_id.as_ref().is_some_and(|id|ids.contains(id))=>{attention_changed=true;break;},
                                Err(RecvError::Lagged(_))=>{attention_changed=true;break;},
                                Err(RecvError::Closed)=>return None,
                                _=>{},
                            },
                            event=async {match &mut approvals {Some(events)=>events.recv().await,None=>pending().await}}=>match event {
                                Ok(event) if event.account==actor.account_id && event.session.as_ref().is_some_and(|id|ids.contains(id))=>{attention_changed=true;break;},
                                Ok(event) if event.account.is_empty()=>{attention_changed=true;break;},
                                Err(RecvError::Lagged(_))=>{attention_changed=true;break;},
                                Err(RecvError::Closed)=>approvals=None,
                                _=>{},
                            }
                        }
                    }
                    sleep(Duration::from_millis(150)).await;
                    while let Ok(event) = events.try_recv() {
                        if event.owner == actor.user_id.as_str()
                            && (!ids.contains(&event.event.session_id)
                                || matches!(
                                    event.event.kind.as_str(),
                                    "session" | "changed" | "ended"
                                ))
                        {
                            catalog_changed = true;
                        }
                    }
                }
                if host.auth.validate_principal(&caller).await.is_err() {
                    return None;
                }
                if attention_changed {
                    attention = host.operator_attention(&caller, &actor, &contexts).await;
                }
                let mut snapshot = host.operator_cards(&caller, &contexts, &attention).await;
                snapshot.catalog_changed = catalog_changed;
                sequence += 1;
                let event = SessionEvent {
                    id: format!("{}/operator/{sequence}", snapshot.runtime_epoch),
                    session_id: String::new(),
                    runtime_epoch: snapshot.runtime_epoch.clone(),
                    kind: "operator_snapshot".into(),
                    attempt_id: None,
                    data: to_value(snapshot).ok()?,
                };
                Some((
                    Ok(event),
                    (
                        host,
                        caller,
                        actor,
                        contexts,
                        ids,
                        events,
                        credentials,
                        approvals,
                        attention,
                        tick,
                        sequence,
                    ),
                ))
            },
        )))
    }
}
