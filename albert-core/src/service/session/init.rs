use std::sync::Arc;

use octo_history::{MemoryLimits, SessionId, SqliteHistory};
use rand::random;

use super::{native_routes::NativeSeen, SessionService};
use crate::{
    db::entity::MarkerState, db::error::DatabaseError, db::repo::excursion::discard_expired_in,
    db::repo::marker::all, db::repo::marker::expire_previous_runtime, db::Database,
    history::sessions::SessionHistory, sessions::error::SessionResult,
    sessions::lease::ScopeRegistry, sessions::settings::SessionSettings,
};

impl SessionService {
    pub async fn from_history(
        history: Arc<SqliteHistory>,
        settings: SessionSettings,
    ) -> SessionResult<Self> {
        settings.validate()?;
        let database = Database::from_pool(history.pool().clone()).await?;
        let limits = &settings.limits;
        let router = Arc::new(SessionHistory::new(
            history.clone(),
            MemoryLimits {
                max_sessions: limits.temporary_max_sessions,
                max_turns: limits.temporary_max_turns,
                max_bytes: limits.temporary_max_bytes,
            },
        )?);
        let runtime_epoch = format!("{:032x}", random::<u128>());
        let mut tx = history.begin_journal().await?;
        expire_previous_runtime(&mut tx, &runtime_epoch).await?;
        discard_expired_in(&mut tx).await?;
        tx.commit().await.map_err(DatabaseError::from)?;
        let leases = Arc::new(ScopeRegistry::default());
        for marker in all(database.pool()).await? {
            leases.restore(
                &SessionId::parse(&marker.session_id)?,
                &marker.runtime_epoch,
                marker.state == MarkerState::Saved && marker.owner_user_id.is_some(),
            )?;
        }
        let service = Self {
            history,
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
