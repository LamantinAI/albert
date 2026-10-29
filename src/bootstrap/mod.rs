//! Startup-only retained history preparation, before any connector accepts input.
mod backup;
mod legacy;
mod web_owner;

use std::{fs::create_dir_all, sync::Arc};

use octo_history::SqliteHistory;
use tracing::{info, warn};

use self::{backup::before_migration, legacy::import_verified, web_owner::attach_configured_owner};
use crate::{
    config::{history::sqlite_path, Config},
    error::{Error, Result},
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
    attach_configured_owner(&service, &config.connectors_manifest).await?;
    if let Err(error) =
        import_verified(&service, &config.connectors_manifest, telegram_enabled).await
    {
        match error {
            Error::Config(_) | Error::Toml(_) | Error::Io(_) => {
                warn!("Legacy source migration deferred until connector configuration is repaired.")
            }
            error => return Err(error),
        }
    }
    info!(path = %path.display(), "retained session history and control metadata ready");
    Ok(service)
}

#[cfg(test)]
mod tests {
    use super::web_owner::attach_configured_owner;
    use crate::{
        db::repo::identity::bind, service::session::SessionService,
        sessions::settings::SessionSettings,
    };
    use octo_history::SqliteHistory;
    use sqlx::{query, query_scalar};
    use std::{
        fs::{create_dir_all, write},
        sync::Arc,
    };
    use tempfile::tempdir;

    #[tokio::test]
    async fn ui_first_owner_links_before_tg_import_without_rebinding_legacy_identities() {
        let dir = tempdir().unwrap();
        let config = dir.path().join("config");
        create_dir_all(config.join("connectors/telegram")).unwrap();
        let manifest = config.join("octo.toml");
        write(&manifest, "[connectors]\ndir='connectors'").unwrap();
        write(
            config.join("connectors/telegram/telegram.toml"),
            "[connector]\nid='telegram'\ntype='telegram'\nowner_chat=7",
        )
        .unwrap();
        let service = Arc::new(
            SessionService::from_history(
                Arc::new(
                    SqliteHistory::open_retained(dir.path().join("history.db"))
                        .await
                        .unwrap(),
                ),
                SessionSettings::default(),
            )
            .await
            .unwrap(),
        );
        let mut tx = service.database().pool().begin().await.unwrap();
        bind(&mut tx, "web", "bootstrap-owner", "web-owner")
            .await
            .unwrap();
        query("INSERT INTO albert_login_accounts(id,username,password_hash,role,canonical_user_id) VALUES ('bootstrap-owner','owner','test','owner','web-owner')").execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        attach_configured_owner(&service, &manifest).await.unwrap();
        attach_configured_owner(&service, &manifest).await.unwrap();
        let user:String=query_scalar("SELECT user_id FROM albert_identity_bindings WHERE connector_id='telegram' AND external_actor_id='7'").fetch_one(service.database().pool()).await.unwrap();
        assert_eq!(user, "web-owner");
        let mut tx = service.database().pool().begin().await.unwrap();
        bind(&mut tx, "telegram", "8", "legacy-owner")
            .await
            .unwrap();
        tx.commit().await.unwrap();
        write(
            config.join("connectors/telegram/telegram.toml"),
            "[connector]\nid='telegram'\ntype='telegram'\nowner_chat=8",
        )
        .unwrap();
        attach_configured_owner(&service, &manifest).await.unwrap();
        let user:String=query_scalar("SELECT user_id FROM albert_identity_bindings WHERE connector_id='telegram' AND external_actor_id='8'").fetch_one(service.database().pool()).await.unwrap();
        assert_eq!(user, "legacy-owner");
        let accounts: i64 = query_scalar("SELECT count(*) FROM albert_login_accounts")
            .fetch_one(service.database().pool())
            .await
            .unwrap();
        assert_eq!(accounts, 1);
    }
}
