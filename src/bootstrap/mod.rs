//! Startup-only retained history preparation, before any connector accepts input.
mod backup;
mod legacy;

use std::{fs::create_dir_all, sync::Arc};

use octo_history::SqliteHistory;
use tracing::info;

use self::{backup::before_migration, legacy::import_verified};
use crate::{
    config::{history::sqlite_path, Config},
    error::Result,
    service::session::SessionService,
};

pub(crate) async fn open_sessions(
    config: &Config,
    telegram_enabled: bool,
) -> Result<Arc<SessionService>> {
    let path = sqlite_path(config.history.as_deref(), &config.deploy_dir)?;
    if let Some(parent) = path.parent() {
        create_dir_all(parent)?;
    }
    if let Some(backup) = before_migration(&path, &config.history_backup_dir).await? {
        info!(path = %backup.display(), "consistent internal database backup created before migrations");
    }
    let history = Arc::new(SqliteHistory::open_retained(&path).await?);
    let service = Arc::new(SessionService::from_history(history, config.sessions.clone()).await?);
    import_verified(&service, &config.connectors_manifest, telegram_enabled).await?;
    info!(path = %path.display(), "retained session history and control metadata ready");
    Ok(service)
}
