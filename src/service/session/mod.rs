//! Session orchestration over the authoritative octo-history catalog. Albert owns
//! verified users and private selectors, never a second catalog or transcript.
mod admission;
mod init;
mod lifecycle;
mod messaging;
mod native_routes;
mod navigation;
mod picker;
mod presentation;
mod preview;
pub(crate) use native_routes::NativeRoute;
#[cfg(test)]
mod legacy;
mod migration;
mod reminders;
mod rename;
mod replay;
mod temporary;
pub(crate) mod titles;

use std::sync::Arc;

use self::{native_routes::NativeSeen, titles::TitleJobs};

use octo_history::{
    PageRequest, Session, SessionFilter, SessionId, SessionPage, SessionStore, SqliteHistory,
};

use crate::{
    db::{entity::PrivateRouteKey, error::DatabaseError, Database},
    history::sessions::SessionHistory,
    messaging::transport::AdapterRegistry,
    sessions::{
        error::{SessionError, SessionResult},
        lease::{ScopeRegistry, SessionLease},
        presentation::PresentationRegistry,
        settings::SessionSettings,
        SessionContext,
    },
};

#[derive(Clone)]
pub(crate) struct SessionService {
    history: Arc<SqliteHistory>,
    database: Database,
    settings: SessionSettings,
    router: Arc<SessionHistory>,
    runtime_epoch: String,
    leases: Arc<ScopeRegistry>,
    native_seen: Arc<NativeSeen>,
    adapters: AdapterRegistry,
    presentations: Arc<PresentationRegistry>,
    title_jobs: Arc<TitleJobs>,
}

impl SessionService {
    pub fn presentations(&self) -> &PresentationRegistry {
        &self.presentations
    }
    pub fn delivery_adapters(&self) -> AdapterRegistry {
        self.adapters.clone()
    }
    pub fn history(&self) -> &Arc<SqliteHistory> {
        &self.history
    }
    pub fn database(&self) -> &Database {
        &self.database
    }
    pub fn settings(&self) -> &SessionSettings {
        &self.settings
    }

    pub fn journal_router(&self) -> &Arc<SessionHistory> {
        &self.router
    }

    pub fn scope_registry(&self) -> Arc<ScopeRegistry> {
        self.leases.clone()
    }

    /// Fetch metadata only. Possession of a UUID is never an access permit.
    pub async fn get(&self, context: &SessionContext, id: &SessionId) -> SessionResult<Session> {
        let mut tx = self.history.begin_journal().await?;
        let session = self.catalog_session_in(&mut tx, id).await?;
        context.check(&session)?;
        tx.commit().await.map_err(DatabaseError::from)?;
        Ok(session)
    }

    pub async fn default(&self, context: &SessionContext) -> SessionResult<Session> {
        self.get(context, context.default_id()).await
    }

    pub async fn list_private(
        &self,
        context: &SessionContext,
        page: PageRequest,
    ) -> SessionResult<SessionPage> {
        context.require_private()?;
        if page.limit > self.settings.limits.max_page_size as usize {
            return Err(SessionError::InvalidInput("page exceeds configured limit"));
        }
        let sessions = self
            .history
            .list_sessions(
                &SessionFilter::PrivateOwner(context.private_user_id()?.as_str().into()),
                page,
            )
            .await?;
        // The supported host policy applies to listed metadata as well as opens.
        for session in &sessions.sessions {
            if session.host != "host" {
                return Err(SessionError::UnsupportedHost(session.host.clone()));
            }
        }
        Ok(sessions)
    }
}

fn route_key(context: &SessionContext) -> SessionResult<PrivateRouteKey> {
    Ok(PrivateRouteKey {
        connector_id: context.reply_route().connector().into(),
        channel_id: context.reply_route().channel().into(),
        user_id: context.private_user_id()?.as_str().into(),
    })
}

fn selected(
    context: &SessionContext,
    session: Session,
    revision: i64,
    lease: Arc<SessionLease>,
    cleanup_scope: Option<String>,
) -> SessionContext {
    let mut context = context.clone();
    context.lease = lease;
    context.cleanup_scope = cleanup_scope;
    context.session = session;
    context.selector_revision = Some(revision);
    context
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use octo_history::{
        CreateSession, HistoryStore, PageRequest, SessionAccess, SessionId, SessionJournal,
        SessionOrigin, SessionStore, SqliteHistory, Turn,
    };
    use tempfile::{tempdir, TempDir};

    use super::SessionService;
    use crate::{
        db::error::DatabaseError,
        sessions::{
            error::SessionError,
            settings::{IdentityLink, SessionSettings},
            Admission, AdmissionScope, ExternalIdentity, ReplyRoute, VerifiedLegacyChild,
            VerifiedLegacyImport,
        },
    };

    async fn fixture(settings: SessionSettings) -> (TempDir, SessionService) {
        let directory = tempdir().unwrap();
        let history = Arc::new(
            SqliteHistory::open_retained(directory.path().join("sessions.db"))
                .await
                .unwrap(),
        );
        let service = SessionService::from_history(history, settings)
            .await
            .unwrap();
        (directory, service)
    }

    fn admission(connector: &str, actor: &str, channel: &str, scope: AdmissionScope) -> Admission {
        Admission::new(
            ExternalIdentity::new(connector, actor).unwrap(),
            ReplyRoute::new(connector, channel, Some(format!("request:{actor}"))).unwrap(),
            scope,
            "host",
        )
        .unwrap()
    }

    fn linked_settings() -> SessionSettings {
        SessionSettings {
            identity_links: vec![
                IdentityLink {
                    connector: "telegram".into(),
                    actor: "7".into(),
                    user_id: "canonical-owner".into(),
                },
                IdentityLink {
                    connector: "console".into(),
                    actor: "local".into(),
                    user_id: "canonical-owner".into(),
                },
            ],
            ..SessionSettings::default()
        }
    }

    #[tokio::test]
    async fn explicit_links_allow_private_cross_channel_open_and_preserve_defaults_and_reply() {
        let (_directory, service) = fixture(linked_settings()).await;
        let telegram = service
            .admit(admission("telegram", "7", "7", AdmissionScope::Private))
            .await
            .unwrap();
        let console = service
            .admit(admission(
                "console",
                "local",
                "console",
                AdmissionScope::Private,
            ))
            .await
            .unwrap();
        assert_eq!(telegram.user_id(), console.user_id());
        assert_ne!(telegram.default_id(), console.default_id());
        let created = service.create_private(&telegram, "Research").await.unwrap();
        let opened = service
            .open_private(
                &console,
                created.session_id(),
                console.selector_revision().unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(opened.session_id(), created.session_id());
        assert_eq!(opened.default_id(), console.default_id());
        assert_eq!(opened.reply_route(), console.reply_route());
        assert_eq!(
            service.default(&opened).await.unwrap().id,
            *console.default_id()
        );
        let readmitted = service
            .admit(admission(
                "console",
                "local",
                "console",
                AdmissionScope::Private,
            ))
            .await
            .unwrap();
        assert_eq!(readmitted.session_id(), created.session_id());
        assert_eq!(readmitted.default_id(), console.default_id());
    }

    #[tokio::test]
    async fn unrelated_external_identities_are_not_merged() {
        let (_directory, service) = fixture(SessionSettings::default()).await;
        let a = service
            .admit(admission("telegram", "7", "7", AdmissionScope::Private))
            .await
            .unwrap();
        let b = service
            .admit(admission(
                "console",
                "local",
                "console",
                AdmissionScope::Private,
            ))
            .await
            .unwrap();
        assert_ne!(a.user_id(), b.user_id());
        assert!(matches!(
            service.get(&b, a.session_id()).await,
            Err(SessionError::Denied)
        ));
        assert!(matches!(
            service.open_private(&b, a.session_id(), 1).await,
            Err(SessionError::Denied)
        ));
    }

    #[tokio::test]
    async fn groups_share_one_session_without_sharing_actor_rights() {
        let (_directory, service) = fixture(linked_settings()).await;
        let a = service
            .admit(admission("telegram", "7", "group-a", AdmissionScope::Group))
            .await
            .unwrap();
        let b = service
            .admit(admission("telegram", "8", "group-a", AdmissionScope::Group))
            .await
            .unwrap();
        let anonymous = service
            .admit(
                Admission::group(
                    None,
                    ReplyRoute::new("telegram", "group-a", None).unwrap(),
                    "host",
                )
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(a.session_id(), b.session_id());
        assert_eq!(a.session_id(), anonymous.session_id());
        assert_ne!(a.user_id(), b.user_id());
        assert!(anonymous.user_id().is_none());
        let other = service
            .admit(admission("telegram", "7", "group-b", AdmissionScope::Group))
            .await
            .unwrap();
        let private = service
            .admit(admission("telegram", "7", "7", AdmissionScope::Private))
            .await
            .unwrap();
        assert!(matches!(
            service.get(&other, a.session_id()).await,
            Err(SessionError::Denied)
        ));
        assert!(matches!(
            service.get(&private, a.session_id()).await,
            Err(SessionError::Denied)
        ));
        assert!(matches!(
            service.get(&a, private.session_id()).await,
            Err(SessionError::Denied)
        ));
        assert!(matches!(
            service.open_private(&a, private.session_id(), 1).await,
            Err(SessionError::GroupSelector)
        ));
        assert!(matches!(
            service.list_private(&a, PageRequest::default()).await,
            Err(SessionError::GroupSelector)
        ));
        // A private web origin with the same canonical user still cannot open a
        // group session. Exact transport binding is not overridden by ownership.
        let console = service
            .admit(admission("console", "local", "ui", AdmissionScope::Private))
            .await
            .unwrap();
        assert!(matches!(
            service.get(&console, a.session_id()).await,
            Err(SessionError::Denied)
        ));
    }

    #[tokio::test]
    async fn stale_selector_rolls_back_creation_and_never_moves_the_default() {
        let (_directory, service) = fixture(SessionSettings::default()).await;
        let context = service
            .admit(admission(
                "console",
                "local",
                "console",
                AdmissionScope::Private,
            ))
            .await
            .unwrap();
        let selected = service.create_private(&context, "First").await.unwrap();
        assert!(matches!(
            service.create_private(&context, "Orphan").await,
            Err(SessionError::Database(DatabaseError::RevisionConflict))
        ));
        let page = service
            .list_private(&selected, PageRequest::default())
            .await
            .unwrap();
        assert_eq!(page.sessions.len(), 2);
        assert!(matches!(
            service
                .open_private(&context, context.default_id(), 1)
                .await,
            Err(SessionError::Database(DatabaseError::RevisionConflict))
        ));
        let latest = service
            .admit(admission(
                "console",
                "local",
                "console",
                AdmissionScope::Private,
            ))
            .await
            .unwrap();
        assert_eq!(latest.session_id(), selected.session_id());
        assert_eq!(latest.default_id(), context.default_id());
    }

    #[tokio::test]
    async fn explicit_import_child_failure_rolls_back_root() {
        let (_directory, service) = fixture(SessionSettings::default()).await;
        service
            .history
            .append("7", &[Turn::user("Root legacy")])
            .await
            .unwrap();
        service
            .history
            .append("child/old", &[Turn::assistant("Child legacy")])
            .await
            .unwrap();
        let identity = ExternalIdentity::new("telegram", "7").unwrap();
        let owner = service.resolve_identity(&identity).await.unwrap();
        let inventory = service.history.journal_inventory().await.unwrap();
        let root = inventory
            .iter()
            .find(|entry| entry.key == "7")
            .unwrap()
            .clone();
        let child = inventory
            .iter()
            .find(|entry| entry.key == "child/old")
            .unwrap()
            .clone();
        let mut stale = child.clone();
        stale.fingerprint = "invalid observed fingerprint".into();
        let id = SessionId::new();
        let mut import = VerifiedLegacyImport {
            source: root,
            origin: SessionOrigin {
                connector: "telegram".into(),
                channel: "7".into(),
            },
            session_id: id.clone(),
            access: SessionAccess::Private {
                owner: owner.as_str().into(),
            },
            children: vec![VerifiedLegacyChild {
                source: stale,
                run_id: "opaque/run:id".into(),
            }],
        };
        assert!(service
            .import_verified_legacy(&[import.clone()])
            .await
            .is_err());
        assert!(service.history.get_session(&id).await.unwrap().is_none());
        assert!(service
            .history
            .get_default_session(&import.origin)
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            service
                .history
                .journal_page("7", PageRequest::default())
                .await
                .unwrap()
                .records
                .len(),
            1
        );
        import.children[0].source = child;
        service
            .import_verified_legacy(&[import.clone()])
            .await
            .unwrap();
        let context = service
            .admit(admission("telegram", "7", "7", AdmissionScope::Private))
            .await
            .unwrap();
        assert_eq!(context.session_id(), &id);
        assert_eq!(
            service
                .history
                .session_page(&context.journal(), PageRequest::default())
                .await
                .unwrap()
                .records
                .len(),
            1
        );
        let child_journal = SessionJournal::Child {
            session_id: id,
            run_id: "opaque/run:id".into(),
        };
        assert_eq!(
            service
                .history
                .session_page(&child_journal, PageRequest::default())
                .await
                .unwrap()
                .records
                .len(),
            1
        );
        assert!(service.import_verified_legacy(&[import]).await.unwrap()[0].duplicate);
    }

    #[tokio::test]
    async fn unsupported_hosts_are_rejected_at_admission_and_catalog_access() {
        let (_directory, service) = fixture(SessionSettings::default()).await;
        assert!(matches!(
            Admission::new(
                ExternalIdentity::new("console", "local").unwrap(),
                ReplyRoute::new("console", "console", None).unwrap(),
                AdmissionScope::Private,
                "remote"
            ),
            Err(SessionError::UnsupportedHost(_))
        ));
        let context = service
            .admit(admission(
                "console",
                "local",
                "console",
                AdmissionScope::Private,
            ))
            .await
            .unwrap();
        let mut request = CreateSession::private(context.user_id().unwrap().as_str());
        request.host = "remote".into();
        let remote = service.history.create_session(request).await.unwrap();
        assert!(matches!(
            service.get(&context, &remote.id).await,
            Err(SessionError::UnsupportedHost(_))
        ));
    }
}
