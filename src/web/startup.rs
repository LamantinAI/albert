//! Assembly of the always-available control plane and optional cognition.
mod connectors;

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use octo_connector_scheduler::Scheduler;
use octo_connector_web::WebConnector;
use octo_core::{EventKind, Filter};
use tracing::{info, warn};

use self::connectors::channels;
use super::{
    runtime::{AgentRuntime, RuntimeCogitator},
    HostedPanel, Panel,
};
use crate::{
    bootstrap::open_sessions,
    config::control::ControlSettings,
    error::{Error, Result},
    service::auth::{AuthService, AuthSettings},
};

pub(crate) async fn run(settings: ControlSettings) -> Result<bool> {
    let config = &settings.base;
    let has_telegram = config.secret("OCTO_TELEGRAM_TOKEN").is_some();
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
        let origin = settings
            .web
            .public_origin
            .clone()
            .unwrap_or_else(|| format!("http://{}", settings.web.bind));
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
        let emits = ["chat.message"].map(EventKind::new);
        let accepts = [
            "chat.reply",
            "chat.status",
            "chat.typing",
            "chat.turn.end",
            "albert.session.closed",
        ]
        .map(EventKind::new);
        let web = WebConnector::new(
            "web",
            settings.web.bind,
            Arc::new(HostedPanel(panel, settings.web.max_body_bytes)),
        )
        .with_kinds(emits, accepts)
        .with_event_filter(Filter::by_kind("chat.*").with_kind("albert.session.closed"))
        .with_shutdown_grace(Duration::from_secs(2));
        info!(address=%settings.web.bind,"web panel configured at /ui/");
        builder = builder.add_connector(web);
    } else {
        warn!("web panel disabled by configuration");
    }
    builder.build().run().await?;
    Ok(restart.load(Ordering::Acquire))
}
