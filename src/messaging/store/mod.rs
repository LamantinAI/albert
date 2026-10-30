//! Canonical history + routing metadata commit in the same SQL transaction.
//! A lifecycle lease covers only local validation/commit, never external work.
mod delivery;
mod evidence;
mod inspection;
mod metadata;
mod persistence;
mod reading;
mod recovery;
mod reports;
mod retention;
mod scheduling;
mod work;
use std::sync::Arc;

use octo_history::SqliteHistory;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use super::{contract::ContentRef, policy::MessagingLimits, MessagingResult};
pub use delivery::DeliveryFailure;
#[cfg(test)]
pub use delivery::SendOutcome;
pub use evidence::{DeliveryEvent, DeliveryObservation};
#[cfg(test)]
pub use inspection::{DeliveryState, MessageStatus};
pub use persistence::{decode_record, MessageRecord};
pub use recovery::{RecoveryAction, RecoveryItem};
pub use work::{
    AdmittedWork, ReportAdmission, ReportCommitReceipt, WorkAuthority, WorkCompletion, WorkTerminal,
};
#[derive(Clone)]
pub struct MessageStore {
    history: Arc<SqliteHistory>,
    lifecycle: Arc<RwLock<()>>,
    limits: MessagingLimits,
}
impl MessageStore {
    /// Pass SessionHistory::lifecycle_gate(), not a fresh independent lock.
    /// Database::from_pool(history.pool().clone()) must have migrated this pool.
    pub fn new(
        history: Arc<SqliteHistory>,
        lifecycle: Arc<RwLock<()>>,
        limits: MessagingLimits,
    ) -> MessagingResult<Self> {
        limits.validate()?;
        Ok(Self {
            history,
            lifecycle,
            limits,
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdmissionReceipt {
    pub source: ContentRef,
    pub duplicate: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkState {
    Passive,
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
    Unknown,
}
#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use octo_history::{
        CreateSession, Session, SessionAccess, SessionJournal, SessionKind, SessionStore,
        SqliteHistory,
    };
    use sqlx::{query, query_scalar, sqlite::SqlitePoolOptions};
    use tokio::sync::RwLock;

    use super::{
        DeliveryFailure, DeliveryState, MessageStore, RecoveryAction, SendOutcome, WorkAuthority,
        WorkState,
    };
    use crate::{
        db::Database,
        messaging::{
            contract::{
                ConnectorGrant, ContentRef, ContextAttachment, DelegationCeiling, DestinationSpec,
                DisplayRoute, ExecutionLocation, MessageDraft, MessageId, MessageIntent,
                TrustedSource, WorkRef,
            },
            policy::{MessagingLimits, RouteGuard},
            MessagingError, MessagingResult,
        },
        sessions::{lease::ScopeRegistry, AdmissionScope, ReplyRoute, SessionContext, UserId},
    };

    pub(super) struct Guard;
    impl RouteGuard for Guard {
        fn check(&self, actor: &str, route: &DisplayRoute) -> MessagingResult<()> {
            if actor == "alice" && route.connector == "telegram" && route.channel == "7" {
                Ok(())
            } else {
                Err(MessagingError::Denied)
            }
        }
    }
    pub(super) struct Revoked;
    impl RouteGuard for Revoked {
        fn check(&self, _: &str, _: &DisplayRoute) -> MessagingResult<()> {
            Err(MessagingError::Denied)
        }
    }
    pub(super) async fn setup(
        limits: MessagingLimits,
    ) -> (MessageStore, TrustedSource, TrustedSource) {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let history = Arc::new(SqliteHistory::from_pool(pool.clone()).await.unwrap());
        Database::from_pool(pool).await.unwrap();
        let a = history
            .create_session(CreateSession::private("alice"))
            .await
            .unwrap();
        let b = history
            .create_session(CreateSession::private("alice"))
            .await
            .unwrap();
        let store = MessageStore::new(history, Arc::new(RwLock::new(())), limits).unwrap();
        query("INSERT INTO albert_users(id) VALUES('alice')")
            .execute(store.history.pool())
            .await
            .unwrap();
        select(&store, &b).await;
        (store, source(a), source(b))
    }
    fn source(session: Session) -> TrustedSource {
        let context = SessionContext {
            lease: ScopeRegistry::default().issue(&session.id, None).unwrap(),
            cleanup_scope: None,
            user: Some(UserId::parse("alice").unwrap()),
            identity: None,
            reply: ReplyRoute::new("telegram", "7", None).unwrap(),
            scope: AdmissionScope::Private,
            default_id: session.id.clone(),
            session,
            selector_revision: None,
        };
        TrustedSource::current(
            &context,
            WorkRef::Turn("existing-turn-id".into()),
            ExecutionLocation::default(),
        )
        .unwrap()
    }
    async fn select(store: &MessageStore, session: &Session) {
        query("INSERT INTO albert_private_session_routes(connector_id,channel_id,user_id,session_id,revision) VALUES('telegram','7','alice',?,1) ON CONFLICT(connector_id,channel_id,user_id) DO UPDATE SET session_id=excluded.session_id,revision=revision+1")
            .bind(session.id.as_str()).execute(store.history.pool()).await.unwrap();
    }
    pub(super) fn display() -> DisplayRoute {
        DisplayRoute {
            connector: "telegram".into(),
            channel: "7".into(),
        }
    }
    pub(super) fn fixed(target: &TrustedSource) -> DestinationSpec {
        DestinationSpec::FixedSession {
            session: target.context.session_id().clone(),
            display: None,
        }
    }
    pub(super) fn draft(destination: DestinationSpec, intent: MessageIntent) -> MessageDraft {
        MessageDraft {
            id: MessageId::new(),
            destination,
            intent,
            content: "selected evidence".into(),
            correlation: None,
            causation: None,
            deadline_unix_ms: 1000,
            hops_remaining: 4,
            context: vec![],
            ceiling: if intent == MessageIntent::WorkRequest {
                Some(DelegationCeiling {
                    grants: vec![ConnectorGrant {
                        connector: "tracker".into(),
                        kinds: vec!["issue.read".into()],
                    }],
                    max_tool_calls: 2,
                    max_output_bytes: 1024,
                })
            } else {
                None
            },
        }
    }
    pub(super) async fn messages(store: &MessageStore) -> i64 {
        query_scalar("SELECT COUNT(*) FROM albert_messages")
            .fetch_one(store.history.pool())
            .await
            .unwrap()
    }
    pub(super) async fn journal_len(store: &MessageStore, source: &TrustedSource) -> usize {
        store
            .history
            .load_session(&source.context.journal())
            .await
            .unwrap()
            .len()
    }

    #[tokio::test]
    async fn duplicate_admission_and_questions_wait_for_received_copy() {
        let (store, a, b) = setup(MessagingLimits::default()).await;
        let proposal = draft(fixed(&b), MessageIntent::Question);
        let first = store.send(&a, &proposal, &Guard, 1).await.unwrap();
        let retry = store.send(&a, &proposal, &Guard, 2).await.unwrap();
        assert!(retry.duplicate);
        assert_eq!(first.source, retry.source);
        assert_eq!(messages(&store).await, 1);
        assert_eq!(journal_len(&store, &a).await, 1);
        assert!(store.queued_work(&b, 8).await.unwrap().is_empty());
        assert!(store.claim_work(&b, &proposal.id, &Guard, 2).await.is_err());
        let received = store.dispatch(&proposal.id, &Guard, 3).await.unwrap();
        assert!(!received.needs_physical_send);
        assert_eq!(store.queued_work(&b, 8).await.unwrap().len(), 1);
        let work = store.claim_work(&b, &proposal.id, &Guard, 4).await.unwrap();
        assert!(matches!(work.authority, WorkAuthority::ContextOnly));
        assert_eq!(work.received, received.recipient);
        assert_eq!(work.envelope.actor, "alice");
        assert_eq!(
            work.envelope.return_address.session,
            *a.context.session_id()
        );
        assert!(store.claim_work(&b, &proposal.id, &Guard, 5).await.is_err());
    }

    #[tokio::test]
    async fn notices_and_responses_are_passive_and_work_keeps_explicit_ceiling() {
        let (store, a, b) = setup(MessagingLimits::default()).await;
        for intent in [MessageIntent::Notice, MessageIntent::Response] {
            let proposal = draft(fixed(&b), intent);
            store.send(&a, &proposal, &Guard, 1).await.unwrap();
            store.dispatch(&proposal.id, &Guard, 2).await.unwrap();
            assert_eq!(
                store.status(&a, &proposal.id).await.unwrap().work,
                Some(WorkState::Passive)
            );
            assert!(store.claim_work(&b, &proposal.id, &Guard, 3).await.is_err());
        }
        let proposal = draft(fixed(&b), MessageIntent::WorkRequest);
        store.send(&a, &proposal, &Guard, 1).await.unwrap();
        store.dispatch(&proposal.id, &Guard, 2).await.unwrap();
        let admitted = store.claim_work(&b, &proposal.id, &Guard, 3).await.unwrap();
        match admitted.authority {
            WorkAuthority::Delegated(ceiling) => assert_eq!(Some(ceiling), proposal.ceiling),
            _ => panic!("missing ceiling"),
        }
    }

    #[tokio::test]
    async fn cross_user_group_temporary_and_foreign_context_reject_before_persistence() {
        let (store, a, b) = setup(MessagingLimits::default()).await;
        let other = store
            .history
            .create_session(CreateSession::private("bob"))
            .await
            .unwrap();
        let mut group_request = CreateSession::private("alice");
        group_request.access = SessionAccess::GroupBound {
            connector: "telegram".into(),
            channel: "7".into(),
        };
        let group = store.history.create_session(group_request).await.unwrap();
        let mut temporary = b.context.session().clone();
        temporary.kind = SessionKind::Temporary;
        let temporary_source = source(temporary);
        assert!(matches!(
            store
                .send(
                    &temporary_source,
                    &draft(fixed(&a), MessageIntent::Notice),
                    &Guard,
                    1
                )
                .await,
            Err(MessagingError::SaveRequired)
        ));
        for session in [other.clone(), group] {
            let proposal = draft(
                DestinationSpec::FixedSession {
                    session: session.id,
                    display: None,
                },
                MessageIntent::Notice,
            );
            assert!(matches!(
                store.send(&a, &proposal, &Guard, 1).await,
                Err(MessagingError::Denied)
            ));
        }
        let mut proposal = draft(fixed(&b), MessageIntent::Notice);
        proposal.context.push(ContextAttachment {
            reference: ContentRef {
                journal: SessionJournal::Child {
                    session_id: other.id,
                    run_id: "child".into(),
                },
                record_id: 1,
            },
            through_id: 1,
            excerpt: Some("secret".into()),
        });
        assert!(matches!(
            store.send(&a, &proposal, &Guard, 1).await,
            Err(MessagingError::Denied)
        ));
        query("INSERT INTO albert_session_markers(session_id,runtime_epoch,state,revision) VALUES(?,'epoch','temporary',1)")
            .bind(b.context.session_id().as_str())
            .execute(store.history.pool())
            .await
            .unwrap();
        assert!(matches!(
            store
                .send(&a, &draft(fixed(&b), MessageIntent::Notice), &Guard, 1)
                .await,
            Err(MessagingError::SaveRequired)
        ));
        assert_eq!(messages(&store).await, 0);
        assert_eq!(journal_len(&store, &a).await, 0);
    }

    #[tokio::test]
    async fn source_and_inbox_roll_back_on_injected_sql_failure() {
        let (store, a, b) = setup(MessagingLimits::default()).await;
        query("CREATE TRIGGER reject_inbox BEFORE INSERT ON albert_message_inbox BEGIN SELECT RAISE(ABORT,'injected admission failure'); END")
            .execute(store.history.pool()).await.unwrap();
        let proposal = draft(fixed(&b), MessageIntent::Question);
        assert!(store.send(&a, &proposal, &Guard, 1).await.is_err());
        assert_eq!(messages(&store).await, 0);
        assert_eq!(journal_len(&store, &a).await, 0);
        query("DROP TRIGGER reject_inbox")
            .execute(store.history.pool())
            .await
            .unwrap();
        assert!(
            !store
                .send(&a, &proposal, &Guard, 2)
                .await
                .unwrap()
                .duplicate
        );
    }

    #[tokio::test]
    async fn result_state_and_report_outbox_roll_back_together() {
        let (store, a, b) = setup(MessagingLimits::default()).await;
        let proposal = draft(fixed(&b), MessageIntent::WorkRequest);
        store.send(&a, &proposal, &Guard, 1).await.unwrap();
        store.dispatch(&proposal.id, &Guard, 2).await.unwrap();
        store.claim_work(&b, &proposal.id, &Guard, 3).await.unwrap();
        let before = journal_len(&store, &b).await;
        let mut report = draft(fixed(&a), MessageIntent::Response);
        report.causation = Some(proposal.id.clone());
        query("CREATE TRIGGER reject_outbox BEFORE INSERT ON albert_message_outbox BEGIN SELECT RAISE(ABORT,'injected report failure'); END")
            .execute(store.history.pool()).await.unwrap();
        assert!(store
            .commit_result(&b, &proposal.id, "finished", Some(&report), &Guard, 4)
            .await
            .is_err());
        assert_eq!(journal_len(&store, &b).await, before);
        assert_eq!(messages(&store).await, 1);
        assert_eq!(
            store.status(&a, &proposal.id).await.unwrap().work,
            Some(WorkState::Running)
        );
        query("DROP TRIGGER reject_outbox")
            .execute(store.history.pool())
            .await
            .unwrap();
        let result = store
            .commit_result(&b, &proposal.id, "finished", Some(&report), &Guard, 5)
            .await
            .unwrap();
        assert_eq!(
            store.status(&a, &proposal.id).await.unwrap().result,
            Some(result.clone())
        );
        assert_eq!(
            store
                .commit_result(&b, &proposal.id, "finished", Some(&report), &Guard, 6)
                .await
                .unwrap(),
            result
        );
        assert!(store
            .commit_result(&b, &proposal.id, "different", Some(&report), &Guard, 7)
            .await
            .is_err());
        assert!(store
            .commit_result(&b, &proposal.id, "finished", None, &Guard, 7)
            .await
            .is_err());
        assert_eq!(messages(&store).await, 2);
    }

    #[tokio::test]
    async fn active_dispatch_pins_once_and_retries_ignore_later_selector_state() {
        let (store, a, b) = setup(MessagingLimits::default()).await;
        let c = store
            .history
            .create_session(CreateSession::private("alice"))
            .await
            .unwrap();
        let proposal = draft(
            DestinationSpec::ChannelActive { route: display() },
            MessageIntent::Notice,
        );
        store.send(&a, &proposal, &Guard, 1).await.unwrap();
        assert!(store
            .status(&a, &proposal.id)
            .await
            .unwrap()
            .resolved
            .is_none());
        select(&store, &c).await;
        let received = store.dispatch(&proposal.id, &Guard, 2).await.unwrap();
        assert_eq!(received.target.session, c.id);
        select(&store, b.context.session()).await;
        query("INSERT INTO albert_session_markers(session_id,runtime_epoch,state,revision) VALUES(?,'epoch','temporary',1)")
            .bind(b.context.session_id().as_str())
            .execute(store.history.pool())
            .await
            .unwrap();
        let attempt = store.send_started(&proposal.id, &Guard, 3).await.unwrap();
        assert_eq!(attempt.target.session, c.id);
        store
            .send_outcome(&proposal.id, &attempt.token, SendOutcome::KnownNotSent)
            .await
            .unwrap();
        assert_eq!(
            store
                .dispatch(&proposal.id, &Guard, 4)
                .await
                .unwrap()
                .recipient,
            received.recipient
        );
        let retry = store.send_started(&proposal.id, &Guard, 5).await.unwrap();
        assert_eq!(retry.target.session, c.id);
        assert!(store
            .send_outcome(&proposal.id, &attempt.token, SendOutcome::Delivered)
            .await
            .is_err());
        store
            .send_outcome(&proposal.id, &retry.token, SendOutcome::Delivered)
            .await
            .unwrap();
        assert_eq!(
            store
                .history
                .load_session(&SessionJournal::Root(c.id))
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn fixed_display_does_not_depend_on_active_selected_session() {
        let (store, a, b) = setup(MessagingLimits::default()).await;
        let c = store
            .history
            .create_session(CreateSession::private("alice"))
            .await
            .unwrap();
        select(&store, &c).await;
        query("INSERT INTO albert_session_markers(session_id,runtime_epoch,state,revision) VALUES(?,'epoch','temporary',1)")
            .bind(c.id.as_str())
            .execute(store.history.pool())
            .await
            .unwrap();
        let proposal = draft(
            DestinationSpec::FixedSession {
                session: b.context.session_id().clone(),
                display: Some(display()),
            },
            MessageIntent::Notice,
        );
        store.send(&a, &proposal, &Guard, 1).await.unwrap();
        assert_eq!(
            store
                .dispatch(&proposal.id, &Guard, 2)
                .await
                .unwrap()
                .target
                .session,
            *b.context.session_id()
        );
        store.send_started(&proposal.id, &Guard, 3).await.unwrap();
        let selected: String = query_scalar("SELECT session_id FROM albert_private_session_routes")
            .fetch_one(store.history.pool())
            .await
            .unwrap();
        assert_eq!(selected, c.id.as_str());
    }

    #[tokio::test]
    async fn recovery_distinguishes_queued_running_result_pending_and_started_transport() {
        let (store, a, b) = setup(MessagingLimits::default()).await;
        let queued = draft(fixed(&b), MessageIntent::Question);
        let running = draft(fixed(&b), MessageIntent::WorkRequest);
        let physical = draft(
            DestinationSpec::ChannelActive { route: display() },
            MessageIntent::Notice,
        );
        for proposal in [&queued, &running, &physical] {
            store.send(&a, proposal, &Guard, 1).await.unwrap();
            store.dispatch(&proposal.id, &Guard, 2).await.unwrap();
        }
        store.claim_work(&b, &running.id, &Guard, 3).await.unwrap();
        store.send_started(&physical.id, &Guard, 3).await.unwrap();
        let items = store.recover(20).await.unwrap();
        assert!(items
            .iter()
            .any(|item| item.message_id == queued.id.as_str()
                && item.action == RecoveryAction::RecheckQueued));
        assert!(items
            .iter()
            .any(|item| item.message_id == running.id.as_str()
                && item.action == RecoveryAction::InterruptedUnknown));
        assert!(items
            .iter()
            .any(|item| item.message_id == physical.id.as_str()
                && item.action == RecoveryAction::TransportUnknown));
        assert!(store.claim_work(&b, &running.id, &Guard, 4).await.is_err());
        assert!(store.send_started(&physical.id, &Guard, 4).await.is_err());
        assert!(store.dispatch(&physical.id, &Guard, 4).await.is_err());
        assert_eq!(
            store.status(&a, &physical.id).await.unwrap().delivery,
            DeliveryState::Unknown
        );
    }

    #[tokio::test]
    async fn limits_deadlines_revocation_and_cleanup_are_bounded() {
        let limits = MessagingLimits {
            max_messages: 1,
            ..MessagingLimits::default()
        };
        let (store, a, b) = setup(limits).await;
        let proposal = draft(fixed(&b), MessageIntent::Notice);
        store.send(&a, &proposal, &Guard, 1).await.unwrap();
        assert!(matches!(
            store
                .send(&a, &draft(fixed(&b), MessageIntent::Notice), &Guard, 2)
                .await,
            Err(MessagingError::Limit)
        ));
        store.dispatch(&proposal.id, &Guard, 3).await.unwrap();
        assert_eq!(store.cleanup_terminal(100, 500, 10).await.unwrap(), 0);
        assert_eq!(store.cleanup_terminal(100, 1001, 10).await.unwrap(), 1);
        assert_eq!(journal_len(&store, &a).await, 1);
        assert!(store.send(&a, &proposal, &Guard, 1002).await.is_err());
        let physical = draft(
            DestinationSpec::ChannelActive { route: display() },
            MessageIntent::Notice,
        );
        store.send(&a, &physical, &Guard, 5).await.unwrap();
        assert!(store.dispatch(&physical.id, &Revoked, 6).await.is_err());
        store
            .fail_delivery(&physical.id, DeliveryFailure::Revoked)
            .await
            .unwrap();
        assert_eq!(
            store
                .status(&a, &physical.id)
                .await
                .unwrap()
                .failure_code
                .as_deref(),
            Some("revoked")
        );
    }
}
