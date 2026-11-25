use crate::usage::UsageLedger;
use std::{path::PathBuf, sync::Arc};

use octo_history::{MemoryLimits, SessionId, SqliteHistory};
use rand::random;

use super::{native_routes::NativeSeen, SessionService};
use crate::{
    accounts::AccountAuthority,
    confirmations::ConfirmationGuard,
    credentials::CredentialGuard,
    db::entity::MarkerState,
    db::error::DatabaseError,
    db::repo::excursion::discard_expired_in,
    db::repo::marker::all,
    db::repo::marker::expire_previous_runtime,
    db::Database,
    history::sessions::SessionHistory,
    media::{AttachmentFiles, MediaStore},
    sessions::error::SessionResult,
    sessions::lease::ScopeRegistry,
    sessions::settings::SessionSettings,
};

impl SessionService {
    pub async fn from_history(
        history: Arc<SqliteHistory>,
        settings: SessionSettings,
    ) -> SessionResult<Self> {
        let files = AttachmentFiles::beside_database(history.pool())
            .await
            .map_err(DatabaseError::from)?;
        Self::initialize(history, settings, files).await
    }

    pub async fn from_history_with_attachment_root(
        history: Arc<SqliteHistory>,
        settings: SessionSettings,
        root: PathBuf,
    ) -> SessionResult<Self> {
        Self::initialize(history, settings, Some(AttachmentFiles::new(root))).await
    }

    async fn initialize(
        history: Arc<SqliteHistory>,
        settings: SessionSettings,
        files: Option<AttachmentFiles>,
    ) -> SessionResult<Self> {
        settings.validate()?;
        let database =
            Database::with_attachment_files(history.pool().clone(), files.as_ref()).await?;
        let media = MediaStore::new(history.pool().clone(), settings.media.clone(), files);
        let limits = &settings.limits;
        let router = Arc::new(SessionHistory::new(
            history.clone(),
            MemoryLimits {
                max_sessions: limits.temporary_max_sessions,
                max_turns: limits.temporary_max_turns,
                max_bytes: limits.temporary_max_bytes,
            },
            media,
        )?);
        let runtime_epoch = format!("{:032x}", random::<u128>());
        let mut tx = history.begin_journal().await?;
        expire_previous_runtime(&mut tx, &runtime_epoch).await?;
        discard_expired_in(&mut tx).await?;
        tx.commit().await.map_err(DatabaseError::from)?;
        let leases = Arc::new(ScopeRegistry::new(&settings.limits));
        for marker in all(database.pool()).await? {
            leases.restore(
                &SessionId::parse(&marker.session_id)?,
                &marker.runtime_epoch,
                marker.state == MarkerState::Saved && marker.owner_user_id.is_some(),
            )?;
        }
        let credentials = CredentialGuard::new(settings.credentials.clone());
        let service = Self {
            credentials,
            confirmations: ConfirmationGuard::new(settings.confirmations.clone()),
            accounts: AccountAuthority::new(history.clone(), settings.telegram_login.clone()),
            history,
            usage: UsageLedger::new(&database),
            database,
            settings,
            router,
            runtime_epoch,
            leases,
            native_seen: Arc::new(NativeSeen::default()),
            adapters: Default::default(),
            presentations: Default::default(),
            title_jobs: Default::default(),
        };
        service.recover_web_starts().await?;
        Ok(service)
    }
}
