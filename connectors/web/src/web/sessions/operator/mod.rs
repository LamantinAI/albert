//! Private-session observations. Neither polling nor a mode change starts work.
mod stream;

use std::collections::HashSet;

use albert_core::{service::session::web::WebActor, sessions::SessionContext};

use super::SessionHost;
use crate::{
    api::errors::ApiError, api::errors::ApiResult, api::host::ApprovalOperation,
    api::host::HostResponse, api::models::OperatorCard, api::models::OperatorQuery,
    api::models::OperatorSnapshot, api::principal::Principal,
};

struct Attention {
    state: String,
    approvals: HashSet<String>,
    credentials: HashSet<String>,
}
impl SessionHost {
    async fn operator_contexts(
        &self,
        caller: &Principal,
        query: &OperatorQuery,
    ) -> ApiResult<(WebActor, Vec<SessionContext>)> {
        let actor = self.actor(caller).await?;
        if query.session_ids.len() > 4000 {
            return Err(ApiError::invalid("Too many conversations."));
        }
        let ids: HashSet<_> = query
            .session_ids
            .split(',')
            .filter(|id| !id.is_empty())
            .collect();
        if ids.len() > 100 {
            return Err(ApiError::invalid(
                "At most 100 conversations can be observed.",
            ));
        }
        let mut contexts = Vec::new();
        for id in ids {
            contexts.push(self.resource(&actor, id).await?);
        }
        contexts.sort_by(|a, b| a.session_id().as_str().cmp(b.session_id().as_str()));
        Ok((actor, contexts))
    }
    async fn operator_attention(
        &self,
        caller: &Principal,
        actor: &WebActor,
        contexts: &[SessionContext],
    ) -> Attention {
        let mut attention = Attention {
            state: "restricted".into(),
            approvals: HashSet::new(),
            credentials: HashSet::new(),
        };
        if caller.owner_permit().is_err() {
            return attention;
        }
        attention.state = "available".into();
        for context in contexts {
            if !self
                .runtime
                .sessions
                .credentials()
                .list(
                    &actor.account_id,
                    actor.user_id.as_str(),
                    Some(context.session_id().as_str()),
                )
                .is_empty()
            {
                attention
                    .credentials
                    .insert(context.session_id().to_string());
            }
        }
        let service = self.approvals.read().unwrap().clone();
        match service {
            Some(service) => match service.operation(caller, ApprovalOperation::List).await {
                Ok(HostResponse::Approvals(list)) => {
                    if !list.warnings.is_empty() {
                        attention.state = "unavailable".into();
                    }
                    for item in list.items.into_iter().filter(|item| item.can_decide) {
                        if let Some(id) = item.session_id {
                            attention.approvals.insert(id);
                        }
                    }
                }
                _ => attention.state = "unavailable".into(),
            },
            None => attention.state = "unavailable".into(),
        }
        attention
    }
    async fn operator_cards(
        &self,
        caller: &Principal,
        contexts: &[SessionContext],
        attention: &Attention,
    ) -> OperatorSnapshot {
        let agent = self.runtime.current();
        let owner = caller.owner_permit().is_ok();
        let mut items = Vec::new();
        for context in contexts {
            if !context.authorized().await {
                continue;
            }
            let (running, outcome, active_children) = match &agent {
                Some(agent) => agent.panel_activity(context.session_id(), owner).await,
                None => (false, None, 0),
            };
            let mut steps = self.hub.operator_steps(context.session_id());
            if !running {
                for step in &mut steps {
                    if step.state == "in_progress" {
                        step.state = "returned".into();
                    }
                }
            }
            items.push(OperatorCard {
                session_id: context.session_id().to_string(),
                running,
                outcome,
                active_children,
                steps,
                attention_state: attention.state.clone(),
                approval_pending: (attention.state == "available")
                    .then(|| attention.approvals.contains(context.session_id().as_str())),
                credential_pending: owner.then(|| {
                    attention
                        .credentials
                        .contains(context.session_id().as_str())
                }),
            });
        }
        OperatorSnapshot {
            catalog_changed: false,
            runtime_epoch: self.runtime.status().runtime_epoch,
            items,
        }
    }
    pub async fn operator_snapshot(
        &self,
        caller: &Principal,
        query: &OperatorQuery,
    ) -> ApiResult<OperatorSnapshot> {
        let (actor, contexts) = self.operator_contexts(caller, query).await?;
        let attention = self.operator_attention(caller, &actor, &contexts).await;
        Ok(self.operator_cards(caller, &contexts, &attention).await)
    }
}

#[cfg(test)]
mod tests {
    use super::super::fixture::fixture;
    use crate::api::models::OperatorQuery;
    use futures::StreamExt;
    use octo_history::SessionId;
    use std::time::Duration;
    use tokio::time::timeout;

    #[tokio::test]
    async fn snapshot_and_single_stream_are_scoped_and_do_not_infer_completion_from_idle() {
        let (_dir, host, caller) = fixture().await;
        let actor = host.actor(&caller).await.unwrap();
        let first = host
            .runtime
            .sessions
            .web_create(&actor, "First", false)
            .await
            .unwrap();
        let second = host
            .runtime
            .sessions
            .web_create(&actor, "Second", true)
            .await
            .unwrap();
        let query = OperatorQuery {
            session_ids: format!("{},{},{}", first.id, second.id, first.id),
        };
        let snapshot = host.operator_snapshot(&caller, &query).await.unwrap();
        assert_eq!(snapshot.items.len(), 2);
        assert!(snapshot
            .items
            .iter()
            .all(|card| !card.running && card.outcome.is_none()));
        assert!(snapshot
            .items
            .iter()
            .all(|card| card.approval_pending.is_none()));
        let mut stream = host.operator_stream(caller.clone(), query).await.unwrap();
        let event = timeout(Duration::from_secs(2), stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(event.kind, "operator_snapshot");
        assert_eq!(event.data["items"].as_array().unwrap().len(), 2);
        let forged = OperatorQuery {
            session_ids: SessionId::new().to_string(),
        };
        assert!(host.operator_snapshot(&caller, &forged).await.is_err());
        assert!(host.operator_stream(caller.clone(), forged).await.is_err());
        host.auth
            .revoke_credential(&caller, caller.credential_id())
            .await
            .unwrap();
        host.hub.changed(
            &first.id,
            actor.user_id.as_str(),
            &host.runtime.status().runtime_epoch,
            "changed",
            None,
        );
        assert!(timeout(Duration::from_secs(2), stream.next())
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn an_existing_foreign_session_is_not_an_observable_resource() {
        use crate::api::models::{AccountRole, UserCreate};
        use albert_core::{
            service::session::web::WebActor, sessions::actor::ActorToolPolicy, sessions::UserId,
        };
        let (_dir, host, caller) = fixture().await;
        let other = host
            .auth
            .create_user(
                &caller.owner_permit().unwrap(),
                UserCreate {
                    username: "other-user".into(),
                    password: "other-user-initial-password".into(),
                    role: AccountRole::User,
                },
            )
            .await
            .unwrap();
        let actor = WebActor {
            account_id: other.id,
            user_id: UserId::parse(other.canonical_user_id.as_deref().unwrap()).unwrap(),
            tools: ActorToolPolicy::ChatOnly,
        };
        let session = host
            .runtime
            .sessions
            .web_create(&actor, "Private work", false)
            .await
            .unwrap();
        let query = OperatorQuery {
            session_ids: session.id.to_string(),
        };
        assert!(host.operator_snapshot(&caller, &query).await.is_err());
        assert!(host.operator_stream(caller, query).await.is_err());
    }
}
