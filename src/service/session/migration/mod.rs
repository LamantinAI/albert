//! Automatic legacy cutover. Host startup and verified native admission share
//! the same transaction path; migration never changes transport permissions.
mod children;

use octo_history::{
    CreateSession, JournalKeyInventory, LegacySessionImport, Session, SessionAccess, SessionId,
    SessionOrigin,
};
use sqlx::{Sqlite, Transaction};

use self::children::children_in;
use super::SessionService;
use crate::{
    db::error::DatabaseError,
    sessions::{
        error::{SessionError, SessionResult},
        require_host, Admission, AdmissionScope,
    },
};

impl SessionService {
    /// Trusted startup adapter supplies the historical channel convention. All
    /// selected roots/children/defaults and identity bindings commit together.
    pub async fn migrate_legacy_defaults(&self, admissions: &[Admission]) -> SessionResult<()> {
        if admissions.is_empty() {
            return Ok(());
        }
        let mut tx = self.history.begin_journal().await?;
        let inventory = self.history.journal_inventory_all_in(&mut tx).await?;
        let children = children_in(&self.history, &mut tx, &inventory).await?;
        for admission in admissions {
            require_host(&admission.host)?;
            let origin = admission.route.origin();
            let access = match admission.scope {
                AdmissionScope::Private => {
                    let identity = admission.identity.as_ref().ok_or(SessionError::Denied)?;
                    let user = self.resolve_identity_in(&mut tx, identity).await?;
                    SessionAccess::Private {
                        owner: user.as_str().into(),
                    }
                }
                AdmissionScope::Group => SessionAccess::GroupBound {
                    connector: origin.connector.clone(),
                    channel: origin.channel.clone(),
                },
            };
            let session = self
                .ensure_migrated_default_in(&mut tx, &origin, access, Some(&inventory))
                .await?;
            for child in children.iter().filter(|child| child.origin == origin) {
                self.history
                    .import_child_journal_in(&mut tx, &session.id, &child.run_id, &child.source)
                    .await?;
            }
        }
        tx.commit().await.map_err(DatabaseError::from)?;
        Ok(())
    }

    pub async fn legacy_child_origins(&self) -> SessionResult<Vec<SessionOrigin>> {
        let mut tx = self.history.begin_journal().await?;
        let inventory = self.history.journal_inventory_all_in(&mut tx).await?;
        let children = children_in(&self.history, &mut tx, &inventory).await?;
        tx.commit().await.map_err(DatabaseError::from)?;
        Ok(children.into_iter().map(|child| child.origin).collect())
    }

    pub(super) async fn ensure_migrated_default_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        origin: &SessionOrigin,
        access: SessionAccess,
        observed: Option<&[JournalKeyInventory]>,
    ) -> SessionResult<Session> {
        let reserved =
            origin.channel.starts_with("session/") || origin.channel.starts_with("subagent/");
        let session =
            if let Some(existing) = self.history.get_default_session_in(tx, origin).await? {
                if existing.access != access {
                    return Err(SessionError::Denied);
                }
                existing
            } else {
                let mut request = CreateSession::private("");
                request.access = access;
                request.title = "Standard".into();
                let source = self
                    .history
                    .journal_inventory_in(tx, &origin.channel)
                    .await?;
                if reserved || source.turns + source.compacts + source.receipts == 0 {
                    self.history
                        .ensure_default_session_in(tx, origin, request)
                        .await?
                } else {
                    request.id = Some(SessionId::new());
                    self.history
                        .import_legacy_sessions_in(
                            tx,
                            &[LegacySessionImport {
                                source,
                                origin: origin.clone(),
                                session: request,
                            }],
                        )
                        .await?
                        .pop()
                        .ok_or(SessionError::Unavailable)?
                        .session
                }
            };
        if observed.is_none() && !reserved {
            let inventory = self
                .history
                .journal_inventory_prefix_in(tx, "subagent/")
                .await?;
            for child in children_in(&self.history, tx, &inventory)
                .await?
                .into_iter()
                .filter(|child| &child.origin == origin)
            {
                self.history
                    .import_child_journal_in(tx, &session.id, &child.run_id, &child.source)
                    .await?;
            }
        }
        Ok(session)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use octo_history::{
        AppendRequest, CreateSession, HistoryStore, PageRequest, SessionJournal, SessionStore,
        SqliteHistory, Turn,
    };
    use serde_json::{from_value, json};
    use sqlx::{query, query_scalar};
    use tempfile::{tempdir, TempDir};
    use tokio::join;

    use crate::{
        service::session::SessionService,
        sessions::{
            settings::SessionSettings, Admission, AdmissionScope, ExternalIdentity, ReplyRoute,
        },
        subagents::Task,
    };

    async fn fixture() -> (TempDir, SessionService) {
        let dir = tempdir().unwrap();
        let history = Arc::new(
            SqliteHistory::open_retained(dir.path().join("history.db"))
                .await
                .unwrap(),
        );
        let service = SessionService::from_history(history, SessionSettings::default())
            .await
            .unwrap();
        (dir, service)
    }
    fn private(channel: &str, actor: &str) -> Admission {
        Admission::new(
            ExternalIdentity::new("telegram", actor).unwrap(),
            ReplyRoute::new("telegram", channel, None).unwrap(),
            AdmissionScope::Private,
            "host",
        )
        .unwrap()
    }
    fn assignment() -> Task {
        from_value(json!({"task":"inspect historical context","context":"explicit context","models":["retired-model"],"role":"historical-role","tools":["historical-tool"]})).unwrap()
    }
    async fn seed(service: &SessionService, channel: &str) -> (AppendRequest, String) {
        let request = AppendRequest {
            namespace: "legacy".into(),
            idempotency_key: channel.into(),
            digest: "original body".into(),
            turns: vec![Turn::user("old input"), Turn::assistant("old answer")],
        };
        service
            .history
            .append_journal(channel, &request)
            .await
            .unwrap();
        let run = "albert/child-1-lazy".to_string();
        service.history.append(&format!("subagent/{run}"), &[Turn::user(json!({"run_id":run,"parent_run_id":"albert/root","origin_connector":"telegram","origin_channel":channel,"assignment":assignment()}).to_string()), Turn::assistant("old child answer")]).await.unwrap();
        (request, run)
    }

    #[tokio::test]
    async fn first_verified_admissions_migrate_once_with_children_receipts_and_compact() {
        let (_dir, service) = fixture().await;
        let (request, run) = seed(&service, "legacy-private-channel").await;
        let old = service
            .history
            .journal_page("legacy-private-channel", PageRequest::default())
            .await
            .unwrap();
        assert!(service
            .history
            .save_compact(
                "legacy-private-channel",
                None,
                old.through_id,
                "complete old context"
            )
            .await
            .unwrap());
        let (first, second) = join!(
            service.admit(private("legacy-private-channel", "7")),
            service.admit(private("legacy-private-channel", "7"))
        );
        let first = first.unwrap();
        assert_eq!(first.session_id(), second.unwrap().session_id());
        let context = service
            .history
            .session_context(&first.journal())
            .await
            .unwrap();
        assert!(context.messages.is_empty());
        assert_eq!(context.compact.unwrap().content, "complete old context");
        let receipt = service
            .history
            .append_session(&first.journal(), &request)
            .await
            .unwrap();
        assert!(receipt.duplicate);
        assert_eq!(
            receipt.record_ids,
            old.records.iter().map(|r| r.id).collect::<Vec<_>>()
        );
        assert_eq!(
            service
                .history
                .load_session(&SessionJournal::Child {
                    session_id: first.session_id().clone(),
                    run_id: run
                })
                .await
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            service
                .history
                .load_session(&first.journal())
                .await
                .unwrap()
                .len(),
            2
        );
        let reopened = service
            .admit(private("legacy-private-channel", "7"))
            .await
            .unwrap();
        assert_eq!(reopened.session_id(), first.session_id());
    }

    #[tokio::test]
    async fn lazy_child_failure_rolls_back_root_identity_default_and_selector() {
        let (_dir, service) = fixture().await;
        seed(&service, "lazy-room").await;
        query("CREATE TRIGGER refuse_child BEFORE UPDATE OF channel ON turns WHEN OLD.channel LIKE 'subagent/%' BEGIN SELECT RAISE(ABORT,'child failure'); END").execute(service.history.pool()).await.unwrap();
        assert!(service.admit(private("lazy-room", "7")).await.is_err());
        let origin = private("lazy-room", "7").route.origin();
        assert!(service
            .history
            .get_default_session(&origin)
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            service
                .history
                .journal_page("lazy-room", PageRequest::default())
                .await
                .unwrap()
                .records
                .len(),
            2
        );
        let identities: i64 = query_scalar("SELECT COUNT(*) FROM albert_identity_bindings")
            .fetch_one(service.history.pool())
            .await
            .unwrap();
        assert_eq!(identities, 0);
        query("DROP TRIGGER refuse_child")
            .execute(service.history.pool())
            .await
            .unwrap();
        assert!(service.admit(private("lazy-room", "7")).await.is_ok());
    }

    #[tokio::test]
    async fn reserved_physical_channel_names_never_steal_session_or_child_journals() {
        let (_dir, service) = fixture().await;
        let existing = service
            .history
            .create_session(CreateSession::private("someone-else"))
            .await
            .unwrap();
        let root = SessionJournal::Root(existing.id);
        let key = root.storage_key().unwrap();
        service
            .history
            .append_session(
                &root,
                &AppendRequest {
                    namespace: "host".into(),
                    idempotency_key: "one".into(),
                    digest: "digest".into(),
                    turns: vec![Turn::user("someone else's content")],
                },
            )
            .await
            .unwrap();
        service
            .history
            .append(
                "subagent/albert/child-reserved",
                &[Turn::user("reserved child")],
            )
            .await
            .unwrap();
        for channel in [&key, "subagent/albert/child-reserved"] {
            let context = service.admit(private(channel, "7")).await.unwrap();
            assert!(service
                .history
                .load_session(&context.journal())
                .await
                .unwrap()
                .is_empty());
        }
        assert_eq!(
            service.history.load_session(&root).await.unwrap()[0].content,
            "someone else's content"
        );
        assert_eq!(
            service
                .history
                .journal_page("subagent/albert/child-reserved", PageRequest::default())
                .await
                .unwrap()
                .records
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn child_only_unknown_origin_and_existing_default_attach_once_without_root_data() {
        let (_dir, service) = fixture().await;
        for (channel, before_default) in [("child-only-room", false), ("existing-room", true)] {
            let existing = if before_default {
                Some(service.admit(private(channel, "7")).await.unwrap())
            } else {
                None
            };
            let run = format!("albert/child-{channel}");
            service.history.append(&format!("subagent/{run}"), &[Turn::user(json!({"run_id":run,"parent_run_id":"albert/root","origin_connector":"telegram","origin_channel":channel,"assignment":assignment()}).to_string()), Turn::assistant("retained child")]).await.unwrap();
            let context = service.admit(private(channel, "7")).await.unwrap();
            if let Some(existing) = existing {
                assert_eq!(context.session_id(), existing.session_id());
            }
            let child = SessionJournal::Child {
                session_id: context.session_id().clone(),
                run_id: run,
            };
            let before = service
                .history
                .session_page(&child, PageRequest::default())
                .await
                .unwrap();
            assert_eq!(before.records.len(), 2);
            assert!(service
                .history
                .load_session(&context.journal())
                .await
                .unwrap()
                .is_empty());
            let again = service.admit(private(channel, "7")).await.unwrap();
            assert_eq!(again.session_id(), context.session_id());
            assert_eq!(
                service
                    .history
                    .session_page(&child, PageRequest::default())
                    .await
                    .unwrap()
                    .records
                    .iter()
                    .map(|r| r.id)
                    .collect::<Vec<_>>(),
                before.records.iter().map(|r| r.id).collect::<Vec<_>>()
            );
        }
    }

    #[tokio::test]
    async fn historical_assignment_shape_is_minimal_and_blank_or_missing_tasks_remain_unbound() {
        let (_dir, service) = fixture().await;
        let shapes = [
            (json!("legacy task text"), true),
            (
                json!({"task":"old task","unknown_old_options":{"model":"retired"}}),
                true,
            ),
            (json!({"task":"   "}), false),
            (json!({"context":"no task field"}), false),
            (json!(null), false),
        ];
        for (index, (assignment, _)) in shapes.iter().enumerate() {
            let run = format!("albert/child-shape-{index}");
            service.history.append(&format!("subagent/{run}"), &[Turn::user(json!({"run_id":run,"parent_run_id":"albert/root","origin_connector":"telegram","origin_channel":"task-shapes","assignment":assignment}).to_string())]).await.unwrap();
        }
        let context = service.admit(private("task-shapes", "7")).await.unwrap();
        for (index, (_, accepted)) in shapes.iter().enumerate() {
            let run = format!("albert/child-shape-{index}");
            if *accepted {
                assert_eq!(
                    service
                        .history
                        .load_session(&SessionJournal::Child {
                            session_id: context.session_id().clone(),
                            run_id: run
                        })
                        .await
                        .unwrap()
                        .len(),
                    1
                );
            } else {
                assert_eq!(
                    service
                        .history
                        .journal_page(&format!("subagent/{run}"), PageRequest::default())
                        .await
                        .unwrap()
                        .records
                        .len(),
                    1
                );
            }
        }
    }
}
