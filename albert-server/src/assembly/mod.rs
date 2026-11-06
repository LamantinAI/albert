//! Assembly of the always-available control plane and optional cognition.
mod connectors;
mod report_factory;

use std::{sync::atomic::AtomicBool, sync::atomic::Ordering, sync::Arc, time::Duration};

use albert_connector_telegram::TelegramTransport;
use albert_connector_web::{HostedPanel, Panel};
use albert_core::{error::Error, error::Result, runtime::AgentRuntime, runtime::RuntimeCogitator};
use octo_connector_scheduler::Scheduler;
use octo_connector_web::WebConnector;
use octo_core::{EventKind, Filter};
use tracing::{info, warn};

use self::connectors::{channels, configured_telegram};
use crate::{bootstrap::open_sessions, settings::ControlSettings};

pub(crate) async fn run(settings: ControlSettings) -> Result<bool> {
    let config = &settings.base;
    let has_telegram = configured_telegram(config);
    let sessions = open_sessions(config, has_telegram).await?;
    let authority = config
        .authorization
        .build()
        .with_scope_validator(sessions.scope_registry());
    let restart = Arc::new(AtomicBool::new(false));
    let runtime = AgentRuntime::new(
        settings.path.clone(),
        config.clone(),
        sessions.clone(),
        authority.clone(),
        restart.clone(),
        Arc::new(TelegramTransport),
    );
    let mut builder = channels(
        config,
        sessions.clone(),
        authority,
        runtime.subscription.clone(),
        has_telegram,
    )?
    .cogitator(Arc::new(RuntimeCogitator(runtime.clone())))
    .add_connector(Scheduler::with_timezone(
        "scheduler",
        config.scheduler_state_path.clone(),
        config.timezone.name(),
    ));
    if settings.web.enabled {
        let origin = settings.web.public_origin.clone().unwrap_or_default();
        let auth = AuthService::open(
            sessions.database().pool().clone(),
            config.deploy_dir.join("state/web-signing.key"),
            AuthSettings {
                secure_cookie: origin.starts_with("https://"),
                public_origin: origin,
                credential_ttl_seconds: settings.web.credential_ttl_seconds,
                login_attempts: settings.web.login_attempts,
                login_window_seconds: settings.web.login_window_seconds,
                password_workers: settings.web.password_workers,
            },
        )
        .await
        .map_err(|_| Error::Config("Web authentication could not be initialized.".into()))?;
        let panel = Panel::new(runtime, auth);
        let emits = ["chat.message", "chat.send_file.result"].map(EventKind::new);
        let accepts = [
            "chat.reply",
            "chat.send_file",
            "chat.status",
            "chat.typing",
            "chat.turn.end",
            "albert.session.closed",
            "albert.progress",
            "albert.text",
            "albert.session.updated",
        ]
        .map(EventKind::new);
        let web = WebConnector::new(
            "web",
            settings.web.bind,
            Arc::new(HostedPanel {
                panel,
                max_body_bytes: settings.web.max_body_bytes,
            }),
        )
        .with_kinds(emits, accepts)
        .with_event_filter(
            Filter::by_kind("chat.*")
                .with_kind("albert.session.closed")
                .with_kind("albert.progress")
                .with_kind("albert.text")
                .with_kind("albert.session.updated"),
        )
        .with_shutdown_grace(Duration::from_secs(2));
        info!(address=%settings.web.bind,"web panel configured at /ui/");
        builder = builder.add_connector(web);
    } else {
        warn!("web panel disabled by configuration");
    }
    builder.build().run().await?;
    Ok(restart.load(Ordering::Acquire))
}

use albert_connector_web::{AuthService, AuthSettings};

#[cfg(test)]
mod tests {
    use std::fs::{create_dir_all, write};

    use albert_core::config::Config;
    use tempfile::tempdir;

    use super::configured_telegram;

    #[test]
    fn telegram_detection_uses_the_connectors_own_credential_reference() {
        let dir = tempdir().unwrap();
        let config = Config::load_control_from(&dir.path().join("albert.toml")).unwrap();
        let manifest = &config.connectors_manifest;
        let parent = manifest.parent().unwrap();
        create_dir_all(parent.join("connectors/private-bot")).unwrap();
        write(manifest, "[connectors]\ndir='connectors'").unwrap();
        write(parent.join("connectors/private-bot/private-bot.toml"), "[connector]\nid='private-bot'\ntype='telegram'\ntoken_env='FIXTURE_NAMED_TG_CREDENTIAL'").unwrap();
        write(
            dir.path().join(".env"),
            "FIXTURE_NAMED_TG_CREDENTIAL=123456789:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n",
        )
        .unwrap();
        assert!(configured_telegram(&config));
        write(dir.path().join(".env"), "FIXTURE_NAMED_TG_CREDENTIAL=\n").unwrap();
        assert!(!configured_telegram(&config));
    }
}
