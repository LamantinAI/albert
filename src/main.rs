//! Albert — the assembly. Octo runtime + an LLM cogitator with kaeru memory and a
//! scheduler connector. Config lives in a TOML file (path via `ALBERT_CONFIG`);
//! secrets stay in `.env`. Talk to it (Telegram if a token is set, else console);
//! it remembers and it reminds.

mod acl;
mod codex_http;
mod codex_model;
mod cogitator;
mod config;
mod console;
mod error;
mod gcal;
mod history;
mod openai_auth;
mod openai_login;
mod prompt;
mod routines;
mod scratchpad;
mod skills;
mod status;

use std::{env::var, sync::Arc};

use dotenvy::{dotenv, from_path};
use kaeru_core::{KaeruConfig, Store};
use kaeru_rig::KaeruMemory;
use octo_connector_caldav::factory as caldav_factory;
use octo_connector_scheduler::Scheduler;
use octo_connector_telegram::factory as telegram_factory;
use octo_core::Octo;
use tracing_subscriber::{fmt, EnvFilter};

use crate::{
    cogitator::AlbertCogitator,
    config::{AuthMode, Config},
    console::ConsoleConnector,
    error::{Error, Result},
    history::{FileHistory, HistoryStore, InMemoryHistory},
    prompt::PromptFiles,
    scratchpad::ScratchpadStore,
    skills::SkillStore,
};

/// Repo-root `.env`, anchored on the manifest so cwd doesn't matter (the crate
/// lives in `albert/`, the `.env` one level up at the repo root — mirrors octo).
const DOTENV_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../.env");

#[tokio::main]
async fn main() -> Result<()> {
    let _ = from_path(DOTENV_PATH);
    let _ = dotenv();

    fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            "albert=info,octo_rig=info,octo_connector_scheduler=info,\
             octo_connector_telegram=info,octo_core=warn"
                .into()
        }))
        .init();

    let config = Config::load()?;
    match config.auth {
        AuthMode::Subscription => eprintln!(
            "[albert] model={} auth=subscription base_url={}",
            config.model, config.subscription_base_url
        ),
        AuthMode::ApiKey => eprintln!(
            "[albert] model={} auth=api_key base_url={}",
            config.model,
            if config.base_url.is_empty() { "(provider default)" } else { &config.base_url }
        ),
    }

    // `albert login` — interactive ChatGPT-subscription sign-in, then exit (no
    // runtime, no memory vault needed). Writes tokens to the subscription store.
    if std::env::args().nth(1).as_deref() == Some("login") {
        eprintln!("[albert] login: writing tokens to {}", config.subscription_auth_json.display());
        return openai_login::run(&config.subscription_auth_json).await;
    }

    // ── Memory: kaeru, scoped to the "albert" initiative ─────────────────────
    let kcfg = KaeruConfig::from_env().map_err(|e| Error::Kaeru(e.to_string()))?;
    let store = Store::open_with_config(kcfg).map_err(|e| Error::Kaeru(e.to_string()))?;
    let memory = KaeruMemory::with_initiative(Arc::new(store), "albert");
    eprintln!("[albert] memory: kaeru (initiative=albert)");

    // ── Hot context: per-channel transcript backend ──────────────────────────
    const HISTORY_MAX: usize = 30;
    let history: Arc<dyn HistoryStore> = match config.history.as_deref() {
        Some(spec) if spec.starts_with("file:") => {
            let dir = &spec["file:".len()..];
            eprintln!("[albert] history: file ({dir})");
            Arc::new(FileHistory::new(dir, HISTORY_MAX)?)
        }
        _ => {
            eprintln!("[albert] history: in-memory");
            Arc::new(InMemoryHistory::new(HISTORY_MAX))
        }
    };

    // ── Scheduler connector (cron/reminders) ─────────────────────────────────
    let scheduler = Scheduler::new("scheduler", config.scheduler_state_path.clone());
    eprintln!("[albert] scheduler: {}", config.scheduler_state_path.display());

    // ── Persona + instructions (RAM, hot-reloaded) ───────────────────────────
    let prompt = PromptFiles::new(config.soul_path.clone(), config.system_path.clone());
    eprintln!(
        "[albert] prompt: soul={} system={}",
        config.soul_path.display(),
        config.system_path.display()
    );

    // ── Loop scratchpad: super-operational per-task working object ───────────
    let scratchpad = ScratchpadStore::new();

    // ── Declarative skills: a folder Albert lists + applies (LRU-cached) ─────
    let skills = SkillStore::load(config.skills_dir.clone(), config.skills_cache);
    eprintln!("[albert] skills: {}", config.skills_dir.display());

    let mut builder = Octo::builder()
        .cogitator(AlbertCogitator::new(
            "albert",
            config.clone(),
            history,
            memory,
            scratchpad,
            skills,
            prompt,
        ))
        .add_connector(scheduler);

    // ── Connectors: config-driven Telegram (ACL) + calendar, or console ──────
    // With a token present, the Telegram channel and the calendar are assembled
    // from config/connectors/*/*.toml via their factories (secrets stay in env,
    // named in each manifest; owner_chat + the ACL live in telegram's manifest).
    // Otherwise a console channel (no calendar in that dev mode).
    let has_telegram = var("OCTO_TELEGRAM_TOKEN").map(|t| !t.trim().is_empty()).unwrap_or(false);
    if has_telegram {
        eprintln!("[albert] channels: telegram (config-driven, ACL) + calendar");
        builder = builder
            .register_connector_type("telegram", telegram_factory())
            .register_connector_type("caldav", caldav_factory())
            .from_config_file(&config.connectors_manifest)?;
    } else {
        eprintln!("[albert] channel: console (set OCTO_TELEGRAM_TOKEN for telegram + calendar)");
        builder = builder.add_connector(ConsoleConnector::new("console"));
    }

    builder.build().run().await?;
    Ok(())
}
