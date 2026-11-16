//! Owner decisions share presentation and lifecycle, while domain adapters keep authority.
mod conversation;
mod handle;
mod index;
mod native;
mod observe;
mod origin;
mod queries;
mod sql;

use std::{collections::HashMap, sync::Arc, sync::Mutex, sync::RwLock};

use self::handle::ApprovalHandle;
use albert_core::runtime::AgentRuntime;
use octo_core::ConnectorContext;
use tokio::sync::broadcast::{channel, Receiver, Sender};
use tokio::{select, spawn, sync::broadcast::error::RecvError};

use self::{index::Entry, native::NativeAdapters, sql::SqlAdapter};
use crate::api::{errors::ApiError, errors::ApiResult};

#[derive(Clone)]
pub(super) struct ApprovalChange {
    pub account: String,
    pub session: Option<String>,
    pub id: String,
}
pub(crate) struct ApprovalService {
    runtime: Arc<AgentRuntime>,
    context: RwLock<Option<ConnectorContext>>,
    adapters: NativeAdapters,
    entries: Mutex<HashMap<(String, String, String), Entry>>,
    changed: Sender<ApprovalChange>,
}
impl ApprovalService {
    pub fn new(runtime: Arc<AgentRuntime>) -> Self {
        let adapter = SqlAdapter::new(runtime.clone());
        Self::with_adapters(runtime, vec![Arc::new(adapter)])
    }
    fn with_adapters(runtime: Arc<AgentRuntime>, adapters: NativeAdapters) -> Self {
        let (changed, _) = channel(128);
        Self {
            runtime,
            context: RwLock::new(None),
            adapters,
            entries: Mutex::new(HashMap::new()),
            changed,
        }
    }
    pub fn attach(&self, context: ConnectorContext) {
        *self.context.write().unwrap() = Some(context.clone());
        let mut events = self.runtime.sessions.confirmations().subscribe();
        let changed = self.changed.clone();
        spawn(async move {
            loop {
                select! {
                    _=context.shutdown.cancelled()=>break,
                    event=events.recv()=>match event {
                        Ok(event)=>{let _=changed.send(ApprovalChange{account:event.account_id,session:Some(event.session_id),id:ApprovalHandle::Confirmation{request:event.request_id}.encode()});},
                        Err(RecvError::Lagged(_))=>{},
                        Err(RecvError::Closed)=>break,
                    }
                }
            }
        });
    }
    pub(super) fn subscribe(&self) -> Receiver<ApprovalChange> {
        self.changed.subscribe()
    }
    fn context(&self) -> ApiResult<ConnectorContext> {
        self.context
            .read()
            .unwrap()
            .clone()
            .ok_or_else(|| ApiError::unavailable("Transport is starting."))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        handle::ApprovalHandle, native::ApprovalAdapter, native::ApprovalOwner,
        native::NativeRecord, ApprovalService,
    };
    use crate::{
        api::errors::ApiError, api::errors::ApiResult, api::models::ApprovalState,
        web::testing::fixture,
    };
    use albert_core::{
        service::session::web::WebActor, sessions::actor::ActorToolPolicy, sessions::UserId,
    };
    use async_trait::async_trait;
    use chrono::Utc;
    use octo_approval::ScopeBinding;
    use octo_core::{ConnectorContext, ConnectorId, Envelope, EventKind, InProcessBus};
    use octo_history::SessionId;
    use sqlx::query;
    use std::time::Duration;
    use std::{sync::atomic::AtomicUsize, sync::atomic::Ordering, sync::Arc, sync::Mutex};
    use tokio::time::timeout;
    use tokio::{spawn, sync::Notify};

    struct NativeFixture {
        record: Mutex<NativeRecord>,
        count: AtomicUsize,
        started: Notify,
        release: Notify,
    }
    #[async_trait]
    impl ApprovalAdapter for NativeFixture {
        fn id(&self) -> &'static str {
            "fixture-native"
        }
        fn observe(&self, event: &Envelope) -> Option<NativeRecord> {
            (event.kind.as_str() == "fixture.approval").then(|| self.record.lock().unwrap().clone())
        }
        async fn list(
            &self,
            _ctx: &ConnectorContext,
            _owner: &ApprovalOwner,
        ) -> ApiResult<Vec<NativeRecord>> {
            Ok(vec![])
        }
        async fn get(
            &self,
            _ctx: &ConnectorContext,
            _owner: &ApprovalOwner,
            _executor: &str,
            _operation: &str,
            _details: bool,
        ) -> ApiResult<NativeRecord> {
            Ok(self.record.lock().unwrap().clone())
        }
        async fn decide(
            &self,
            _ctx: &ConnectorContext,
            _owner: &ApprovalOwner,
            _record: &NativeRecord,
            _approve: bool,
        ) -> ApiResult<NativeRecord> {
            self.count.fetch_add(1, Ordering::SeqCst);
            self.started.notify_one();
            self.release.notified().await;
            Err(ApiError::unavailable("The response was lost."))
        }
    }
    async fn native_fixture() -> (
        tempfile::TempDir,
        Arc<ApprovalService>,
        Arc<NativeFixture>,
        ApprovalOwner,
        String,
    ) {
        let (dir, runtime, _auth, caller) = fixture().await;
        let actor = WebActor {
            account_id: caller.account_id().into(),
            user_id: UserId::parse(caller.canonical_user_id().unwrap()).unwrap(),
            tools: ActorToolPolicy::ExistingHost,
        };
        let session = runtime
            .sessions
            .web_create(&actor, "Native approval fixture", false)
            .await
            .unwrap();
        let owner = ApprovalService::new(runtime.clone())
            .owner(&caller)
            .await
            .unwrap();
        let context = runtime
            .sessions
            .web_context(&actor, &session.id)
            .await
            .unwrap()
            .with_account(
                runtime
                    .sessions
                    .accounts()
                    .account(&owner.account)
                    .await
                    .unwrap(),
            );
        let binding = runtime
            .sessions
            .scope_registry()
            .binding(&context)
            .unwrap()
            .unwrap();
        assert_ne!(binding.opaque_id, format!("account/{}", owner.account));
        let record = NativeRecord {
            adapter: "fixture-native".into(),
            executor: "fixture".into(),
            operation: "operation".into(),
            operation_hash: "immutable-operation".into(),
            recipient_principal: owner.origin.principal.clone(),
            origin_scope: Some(binding),
            decision_connector: "web".into(),
            private_destination: format!("user:{}", owner.user),
            state: ApprovalState::Pending,
            title: "Publish the draft?".into(),
            action: "Publish".into(),
            effects: vec!["A draft becomes visible".into()],
            details: None,
            expires_at: Utc::now().timestamp() + 300,
            can_decide: true,
        };
        let native = Arc::new(NativeFixture {
            record: Mutex::new(record),
            count: AtomicUsize::new(0),
            started: Notify::new(),
            release: Notify::new(),
        });
        let service = Arc::new(ApprovalService::with_adapters(
            runtime,
            vec![native.clone()],
        ));
        service.attach(ConnectorContext::new(
            Default::default(),
            Arc::new(InProcessBus::new(32)),
        ));
        let id = native.record.lock().unwrap().handle().encode();
        (dir, service, native, owner, id)
    }
    #[tokio::test]
    async fn duplicate_decisions_dispatch_once_and_unknown_survives_an_empty_queue_until_exact_terminal_status(
    ) {
        let (_dir, service, native, owner, id) = native_fixture().await;
        let first_service = service.clone();
        let first_owner = owner.clone();
        let first_id = id.clone();
        let first = spawn(async move {
            first_service
                .decide(&first_owner, &first_id, true)
                .await
                .unwrap()
        });
        timeout(Duration::from_secs(3), native.started.notified())
            .await
            .unwrap();
        assert_eq!(
            service.decide(&owner, &id, true).await.unwrap().state,
            ApprovalState::Deciding
        );
        assert_eq!(native.count.load(Ordering::SeqCst), 1);
        native.release.notify_one();
        assert_eq!(
            timeout(Duration::from_secs(3), first)
                .await
                .unwrap()
                .unwrap()
                .state,
            ApprovalState::Unknown
        );
        let queue = service.list(&owner, None).await.unwrap();
        assert_eq!(queue.items.len(), 1);
        assert_eq!(queue.items[0].state, ApprovalState::Unknown);
        assert!(!queue.items[0].can_decide);
        assert_eq!(
            service.decide(&owner, &id, true).await.unwrap().state,
            ApprovalState::Unknown
        );
        assert_eq!(native.count.load(Ordering::SeqCst), 1);
        native.record.lock().unwrap().state = ApprovalState::Committed;
        assert_eq!(
            service.get(&owner, &id, false).await.unwrap().state,
            ApprovalState::Committed
        );
        assert_eq!(native.count.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn only_full_owned_origin_bindings_attach_a_request_to_a_chat() {
        let (_dir, service, native, owner, id) = native_fixture().await;
        let view = service.get(&owner, &id, false).await.unwrap();
        let session = view.session_id.unwrap();
        let original = native.record.lock().unwrap().origin_scope.clone().unwrap();
        assert_eq!(
            service
                .origin_session(&owner, Some(&original))
                .await
                .as_deref(),
            Some(session.as_str())
        );
        assert!(service
            .origin_session(
                &owner,
                Some(&ScopeBinding::new("account/foreign", &session).unwrap())
            )
            .await
            .is_none());
        assert!(service
            .origin_session(
                &owner,
                Some(&ScopeBinding::new(format!("account/{}", owner.account), "invalid").unwrap())
            )
            .await
            .is_none());
        assert!(service.origin_session(&owner, None).await.is_none());
        assert_eq!(
            service
                .origin_session(
                    &owner,
                    Some(&ScopeBinding::new(format!("session/{session}"), "legacy-epoch").unwrap())
                )
                .await
                .as_deref(),
            Some(session.as_str())
        );
        let mut foreign = owner.clone();
        foreign.user = "foreign-user".into();
        assert!(service.cached(&foreign, &id).is_none());
        assert!(service.require_session(&foreign, &session).await.is_err());
    }
    #[tokio::test]
    async fn native_identity_changes_fail_closed_and_unchanged_snapshots_do_not_emit_event_loops() {
        let (_dir, service, native, owner, id) = native_fixture().await;
        let mut changes = service.subscribe();
        service.get(&owner, &id, false).await.unwrap();
        assert!(changes.try_recv().is_ok());
        service.get(&owner, &id, false).await.unwrap();
        assert!(changes.try_recv().is_err());
        native.record.lock().unwrap().operation_hash = "different-operation".into();
        assert!(service.get(&owner, &id, false).await.is_err());
        assert!(ApprovalHandle::decode("not-an-approval").is_err());
    }
    #[tokio::test]
    async fn observed_native_request_with_real_grant_scope_notifies_and_lists_its_chat() {
        let (_dir, service, native, owner, id) = native_fixture().await;
        let scope = native.record.lock().unwrap().origin_scope.clone().unwrap();
        assert_eq!(
            service
                .runtime
                .sessions
                .scope_registry()
                .account_for_scope(&scope)
                .as_deref(),
            Some(owner.account.as_str())
        );
        let mut changes = service.subscribe();
        service
            .observe(&Envelope::new(
                ConnectorId::new("fixture"),
                EventKind::new("fixture.approval"),
                "pending protected action".to_owned(),
            ))
            .await;
        let change = timeout(Duration::from_secs(1), changes.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(change.account, owner.account);
        assert_eq!(change.session.as_deref(), Some(scope.epoch.as_str()));
        assert_eq!(change.id, id);
        let scoped = service.list(&owner, Some(&scope.epoch)).await.unwrap();
        assert_eq!(scoped.items.len(), 1);
        assert_eq!(scoped.items[0].id, id);
        assert!(scoped.items[0].can_decide);
        let forged = ScopeBinding::new(format!("account/{}", owner.account), &scope.epoch).unwrap();
        assert!(service
            .origin_session(&owner, Some(&forged))
            .await
            .is_none());
        let mut wrong_session = scope.clone();
        wrong_session.epoch = SessionId::new().to_string();
        assert!(service
            .origin_session(&owner, Some(&wrong_session))
            .await
            .is_none());
        service
            .runtime
            .sessions
            .scope_registry()
            .close(&SessionId::parse(&scope.epoch).unwrap())
            .unwrap();
        assert!(service
            .runtime
            .sessions
            .scope_registry()
            .account_for_scope(&scope)
            .is_none());
        assert!(service.origin_session(&owner, Some(&scope)).await.is_none());
    }

    #[tokio::test]
    async fn current_owner_revocation_blocks_a_native_decision_before_dispatch() {
        let (_dir, service, native, owner, id) = native_fixture().await;
        query("UPDATE albert_login_accounts SET role='user' WHERE id=?")
            .bind(&owner.account)
            .execute(service.runtime.sessions.database().pool())
            .await
            .unwrap();
        assert!(service.decide(&owner, &id, true).await.is_err());
        assert_eq!(native.count.load(Ordering::SeqCst), 0);
    }
}
