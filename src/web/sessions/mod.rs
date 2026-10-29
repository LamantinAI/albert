//! Browser Session resources are explicit and private-user scoped. Transport
//! routes remain provenance; neither a login credential nor a view is a chat.
mod catalog;
mod cursor;
mod events;
#[cfg(test)]
mod fixture;
mod lifecycle;
mod messages;
mod send;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, RwLock, Weak},
};

use axum::http::StatusCode;
use octo_core::{ConnectorContext, Envelope};
use octo_history::{Session, SessionId, SessionKind, SessionState as MetadataState};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

use self::events::EventHub;
use super::runtime::AgentRuntime;
use crate::{
    api::{
        errors::{ApiError, ApiResult},
        host::{EventStream, HostResponse, SessionEventRequest, SessionOperation},
        models::{AccountRole, AttemptView, SessionState, SessionView},
        principal::Principal,
    },
    service::{auth::AuthService, session::web::WebActor},
    sessions::{actor::ActorToolPolicy, error::SessionError, SessionContext, UserId},
};

pub(crate) struct SessionHost {
    runtime: Arc<AgentRuntime>,
    auth: Arc<AuthService>,
    context: RwLock<Option<ConnectorContext>>,
    hub: Arc<EventHub>,
    gates: Mutex<HashMap<SessionId, Weak<AsyncMutex<()>>>>,
}
impl SessionHost {
    pub fn new(runtime: Arc<AgentRuntime>, auth: Arc<AuthService>) -> Arc<Self> {
        Arc::new(Self {
            runtime,
            auth,
            context: RwLock::new(None),
            hub: EventHub::new(),
            gates: Mutex::new(HashMap::new()),
        })
    }
    pub fn attach(&self, context: ConnectorContext) {
        *self.context.write().unwrap() = Some(context);
    }
    async fn actor(&self, caller: &Principal) -> ApiResult<WebActor> {
        let current = self.auth.validate_principal(caller).await?;
        if current.must_rotate() {
            return Err(ApiError::forbidden());
        }
        let user = current
            .canonical_user_id()
            .ok_or_else(|| ApiError::conflict("Choose a profile before opening chats."))?;
        Ok(WebActor {
            account_id: current.account_id().into(),
            user_id: UserId::parse(user).map_err(session_error)?,
            tools: if current.role() == AccountRole::Owner {
                ActorToolPolicy::ExistingHost
            } else {
                ActorToolPolicy::ChatOnly
            },
        })
    }
    async fn gate(&self, id: &SessionId) -> OwnedMutexGuard<()> {
        let lock = {
            let mut gates = self.gates.lock().unwrap();
            gates.retain(|_, gate| gate.strong_count() > 0);
            if let Some(lock) = gates.get(id).and_then(Weak::upgrade) {
                lock
            } else {
                let lock = Arc::new(AsyncMutex::new(()));
                gates.insert(id.clone(), Arc::downgrade(&lock));
                lock
            }
        };
        lock.lock_owned().await
    }
    pub async fn operation(
        &self,
        caller: &Principal,
        operation: SessionOperation,
    ) -> ApiResult<HostResponse> {
        let actor = self.actor(caller).await?;
        match operation {
            SessionOperation::List(page) => {
                Ok(HostResponse::Sessions(self.catalog(&actor, page).await?))
            }
            SessionOperation::Create(input) => {
                let session = self
                    .runtime
                    .sessions
                    .web_create(
                        &actor,
                        input.title.as_deref().unwrap_or(""),
                        input.temporary,
                    )
                    .await
                    .map_err(session_error)?;
                Ok(HostResponse::Session(view(&session)))
            }
            SessionOperation::Get { id } => {
                let context = self.resource(&actor, &id).await?;
                Ok(HostResponse::Session(view(context.session())))
            }
            SessionOperation::Update { id, input } => self.update(&actor, &id, input).await,
            SessionOperation::Messages { id, page } => Ok(HostResponse::Messages(
                self.messages(&actor, &id, page).await?,
            )),
            SessionOperation::Send { id, input } => {
                Ok(HostResponse::Receipt(self.send(&actor, &id, input).await?))
            }
            SessionOperation::Receipt { id, request_id } => Ok(HostResponse::Receipt(
                self.receipt(&actor, &id, &request_id).await?,
            )),
            SessionOperation::State { id } => {
                Ok(HostResponse::State(self.state(&actor, &id).await?))
            }
            SessionOperation::Cancel { attempt_id, input } => {
                let context = self.resource(&actor, &input.session_id).await?;
                let agent = self
                    .runtime
                    .current()
                    .ok_or_else(|| ApiError::unavailable("Connect a model first."))?;
                let runtime = self
                    .runtime
                    .context()
                    .ok_or_else(|| ApiError::unavailable("Runtime is starting."))?;
                let cancelled = agent
                    .panel_cancel_exact(context.session_id(), &attempt_id, &runtime)
                    .await;
                if !cancelled {
                    return Err(ApiError::conflict("This attempt is no longer active."));
                }
                Ok(HostResponse::Attempt(AttemptView {
                    id: attempt_id,
                    state: "cancelled".into(),
                    session_id: context.session_id().to_string(),
                }))
            }
        }
    }
    async fn resource(&self, actor: &WebActor, id: &str) -> ApiResult<SessionContext> {
        let id = SessionId::parse(id).map_err(|_| ApiError::invalid("Invalid Session ID."))?;
        self.runtime
            .sessions
            .web_context(actor, &id)
            .await
            .map_err(session_error)
    }
    async fn state(&self, actor: &WebActor, id: &str) -> ApiResult<SessionState> {
        let context = self.resource(actor, id).await?;
        let active = match self.runtime.current() {
            Some(agent) => agent.panel_active(context.session_id()).await,
            None => None,
        };
        Ok(SessionState {
            session: view(context.session()),
            active_attempt: active.map(|id| AttemptView {
                id,
                state: "running".into(),
                session_id: context.session_id().to_string(),
            }),
            runtime_epoch: self.runtime.status().runtime_epoch,
            status: self.hub.status(context.session_id()),
        })
    }
    pub fn observe(&self, envelope: &Envelope) {
        self.hub
            .observe(envelope, &self.runtime.status().runtime_epoch);
    }
    pub async fn events(
        &self,
        caller: Principal,
        request: SessionEventRequest,
    ) -> ApiResult<EventStream> {
        self.event_stream(caller, request).await
    }
}
pub(super) fn timestamp(value: &str) -> String {
    if value.contains('T') {
        value.into()
    } else {
        format!("{}Z", value.replacen(' ', "T", 1))
    }
}
pub(super) fn view(session: &Session) -> SessionView {
    SessionView {
        id: session.id.to_string(),
        title: session.title.clone(),
        host: session.host.clone(),
        kind: match session.kind {
            SessionKind::Temporary => "temporary",
            SessionKind::Permanent => "permanent",
        }
        .into(),
        state: match session.state {
            MetadataState::Active => "active",
            MetadataState::Archived => "archived",
        }
        .into(),
        revision: session.revision,
        created_at: timestamp(&session.created_at),
    }
}
pub(super) fn session_error(error: SessionError) -> ApiError {
    match error {
        SessionError::Denied | SessionError::GroupSelector => ApiError::forbidden(),
        SessionError::NotFound => ApiError::not_found(),
        SessionError::Ended | SessionError::Expired => ApiError::new(
            StatusCode::GONE,
            "session_ended",
            "This temporary chat has ended or expired.",
        ),
        SessionError::InvalidInput(message) => ApiError::invalid(message),
        _ => {
            ApiError::conflict("The chat changed or could not be updated. Reload before retrying.")
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::http::StatusCode;
    use futures::StreamExt;
    use octo_history::{AppendRequest, HistoryStore, SessionId, Turn};
    use sha2::{Digest, Sha256};
    use tokio::time::timeout;

    use super::{cursor::Cursor, fixture::fixture, send::namespace};
    use crate::api::{
        host::{HostResponse, SessionEventRequest, SessionOperation},
        models::{MessageCreate, PageQuery, SessionUpdate},
    };

    #[tokio::test]
    async fn message_pages_are_descending_snapshot_bound_and_resource_scoped() {
        let (_dir, host, caller) = fixture().await;
        let actor = host.actor(&caller).await.unwrap();
        let session = host
            .runtime
            .sessions
            .web_create(&actor, "Paged", false)
            .await
            .unwrap();
        let context = host.resource(&actor, session.id.as_str()).await.unwrap();
        let journal = context.root_history_key();
        host.runtime
            .sessions
            .journal_router()
            .append_journal(
                &journal,
                &AppendRequest {
                    namespace: "pages".into(),
                    idempotency_key: "initial".into(),
                    digest: "initial".into(),
                    turns: (0..12)
                        .map(|index| Turn::user(format!("message-{index}")))
                        .collect(),
                },
            )
            .await
            .unwrap();
        let page = host
            .messages(&actor, session.id.as_str(), PageQuery::default())
            .await
            .unwrap();
        assert_eq!(page.items.len(), 10);
        assert_eq!(page.items.first().unwrap().content, "message-11");
        assert_eq!(page.items.last().unwrap().content, "message-2");
        host.runtime
            .sessions
            .journal_router()
            .append_journal(
                &journal,
                &AppendRequest {
                    namespace: "pages".into(),
                    idempotency_key: "later".into(),
                    digest: "later".into(),
                    turns: vec![Turn::user("later")],
                },
            )
            .await
            .unwrap();
        let cursor = page.next_cursor.unwrap();
        let page = host
            .messages(
                &actor,
                session.id.as_str(),
                PageQuery {
                    after: Some(cursor.clone()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(
            page.items
                .iter()
                .map(|item| item.content.as_str())
                .collect::<Vec<_>>(),
            ["message-1", "message-0"]
        );
        assert!(!page.has_more);
        let other = host
            .runtime
            .sessions
            .web_create(&actor, "Other", false)
            .await
            .unwrap();
        assert_eq!(
            host.messages(
                &actor,
                other.id.as_str(),
                PageQuery {
                    after: Some(cursor),
                    ..Default::default()
                }
            )
            .await
            .err()
            .unwrap()
            .status,
            StatusCode::BAD_REQUEST
        );
        let foreign = Cursor::Messages {
            owner: "another-owner".into(),
            session: session.id.to_string(),
            before: None,
            through: 12,
        }
        .encode();
        assert_eq!(
            host.messages(
                &actor,
                session.id.as_str(),
                PageQuery {
                    after: Some(foreign),
                    ..Default::default()
                }
            )
            .await
            .err()
            .unwrap()
            .status,
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn lost_receipt_retry_survives_promotion_and_rejects_body_or_chat_rebinding() {
        let (_dir, host, caller) = fixture().await;
        let actor = host.actor(&caller).await.unwrap();
        let session = host
            .runtime
            .sessions
            .web_create(&actor, "Temporary", true)
            .await
            .unwrap();
        let context = host.resource(&actor, session.id.as_str()).await.unwrap();
        let request = SessionId::new().to_string();
        let digest = format!("{:x}", Sha256::digest(b"accepted message"));
        let accepted = host
            .runtime
            .sessions
            .journal_router()
            .append_journal(
                &context.root_history_key(),
                &AppendRequest {
                    namespace: namespace(&actor),
                    idempotency_key: request.clone(),
                    digest,
                    turns: vec![Turn::user("accepted message")],
                },
            )
            .await
            .unwrap();
        let receipt = host
            .send(
                &actor,
                session.id.as_str(),
                MessageCreate {
                    client_message_id: request.clone(),
                    text: "accepted message".into(),
                    digest: None,
                },
            )
            .await
            .unwrap();
        assert!(receipt.duplicate);
        assert_eq!(
            receipt.message_ids,
            accepted
                .record_ids
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        );
        let saved = host
            .operation(
                &caller,
                SessionOperation::Update {
                    id: session.id.to_string(),
                    input: SessionUpdate {
                        revision: session.revision,
                        title: Some(session.title.clone()),
                        archived: None,
                        kind: Some("permanent".into()),
                        state: None,
                        promotion_key: Some("save-once".into()),
                    },
                },
            )
            .await
            .unwrap();
        assert!(matches!(saved, HostResponse::Session(ref view) if view.kind == "permanent"));
        let after = host
            .receipt(&actor, session.id.as_str(), &request)
            .await
            .unwrap();
        assert_eq!(after.message_ids, receipt.message_ids);
        assert_eq!(
            host.send(
                &actor,
                session.id.as_str(),
                MessageCreate {
                    client_message_id: request.clone(),
                    text: "changed body".into(),
                    digest: None,
                }
            )
            .await
            .err()
            .unwrap()
            .status,
            StatusCode::CONFLICT
        );
        let other = host
            .runtime
            .sessions
            .web_create(&actor, "Other", false)
            .await
            .unwrap();
        assert_eq!(
            host.receipt(&actor, other.id.as_str(), &request)
                .await
                .err()
                .unwrap()
                .status,
            StatusCode::CONFLICT
        );
    }

    #[tokio::test]
    async fn event_replay_is_session_scoped_and_end_wakes_idle_subscribers() {
        let (_dir, host, caller) = fixture().await;
        let actor = host.actor(&caller).await.unwrap();
        let session = host
            .runtime
            .sessions
            .web_create(&actor, "Temporary", true)
            .await
            .unwrap();
        let request = || SessionEventRequest {
            session_id: session.id.to_string(),
            after: None,
            runtime_epoch: None,
        };
        let mut stream = host.events(caller.clone(), request()).await.unwrap();
        let initial = timeout(Duration::from_secs(1), stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(initial.kind, "snapshot");
        let epoch = host.runtime.status().runtime_epoch;
        let foreign = SessionId::new();
        host.hub
            .changed(&foreign, actor.user_id.as_str(), &epoch, "changed", None);
        host.hub
            .changed(&session.id, actor.user_id.as_str(), &epoch, "changed", None);
        let changed = timeout(Duration::from_secs(1), stream.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(changed.session_id, session.id.as_str());
        assert_eq!(changed.kind, "changed");
        let mut replay_request = request();
        replay_request.after = Some(initial.id);
        let mut replay = host.events(caller.clone(), replay_request).await.unwrap();
        let replayed = timeout(Duration::from_secs(1), replay.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(replayed.id, changed.id);
        let mut invalid = request();
        invalid.after = Some(format!("{epoch}/{foreign}/1"));
        assert_eq!(
            host.events(caller.clone(), invalid)
                .await
                .err()
                .unwrap()
                .status,
            StatusCode::BAD_REQUEST
        );
        host.operation(
            &caller,
            SessionOperation::Update {
                id: session.id.to_string(),
                input: SessionUpdate {
                    revision: session.revision,
                    title: None,
                    archived: None,
                    kind: None,
                    state: Some("ended".into()),
                    promotion_key: None,
                },
            },
        )
        .await
        .unwrap();
        assert!(timeout(Duration::from_secs(1), stream.next())
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            host.messages(&actor, session.id.as_str(), PageQuery::default())
                .await
                .err()
                .unwrap()
                .status,
            StatusCode::GONE
        );
    }
}
