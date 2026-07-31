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
mod history;
mod openai_auth;
mod openai_login;
mod prompt;
mod routines;
mod scratchpad;
mod selfconfig;
mod skills;
mod transcribe;
mod status;

use std::{collections::HashMap, env::{set_var, var}, fs::create_dir_all, sync::Arc};

use dotenvy::{dotenv, from_path};
use kaeru_core::{KaeruConfig, Store};
use kaeru_rig::{CloudClient, CloudRegistry, KaeruMemory};
use octo_code::WORKSPACE_ENV;
use octo_connector_caldav::factory as caldav_factory;
use octo_connector_forkd::{factory as forkd_factory, SKILLS_ENV};
use octo_connector_http::factory as http_factory;
use octo_connector_mail::{ensure_crypto_provider, factory as mail_factory};
use octo_connector_scheduler::Scheduler;
use octo_connector_search::factory as search_factory;
use octo_connector_storage::factory as storage_factory;
use octo_connector_telegram::factory as telegram_factory;
use octo_core::Octo;
use tracing::{info, warn};
use tracing_subscriber::{fmt, EnvFilter};

use crate::{
    cogitator::AlbertCogitator,
    config::{AuthMode, Config},
    console::ConsoleConnector,
    error::{Error, Result},
    history::{FileHistory, HistoryStore, InMemoryHistory, SqliteHistory},
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

    // tracing-subscriber: level-filtered structured logs. Default shows albert +
    // the noisy connectors at info; override per-target with RUST_LOG, e.g.
    // `RUST_LOG=albert=debug,octo_core=info` for the trace/debug detail below.
    fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            "albert=info,octo_rig=info,octo_connector_scheduler=info,\
             octo_connector_telegram=info,octo_connector_caldav=info,\
             octo_connector_storage=info,octo_connector_forkd=info,\
             octo_connector_mail=info,octo_connector_search=info,\
             octo_connector_http=info,octo_core=warn"
                .into()
        }))
        .with_target(true)
        .init();
    info!("albert starting");

    // Pin the rustls CryptoProvider process-wide BEFORE any TLS config is built.
    // Linking octo-connector-mail brings a second provider (ring) alongside reqwest's
    // aws-lc-rs; with both compiled in, the first `ClientConfig::builder()` without an
    // installed default panics. This must run before caldav's first HTTPS and before
    // the `login` OAuth flow — i.e. here, regardless of whether mail is enabled.
    ensure_crypto_provider();

    let config = Config::load()?;
    match config.auth {
        AuthMode::Subscription => {
            info!(model = %config.model, base_url = %config.subscription_base_url, "llm backend: subscription");
        }
        AuthMode::ApiKey => {
            let base = if config.base_url.is_empty() { "(provider default)" } else { &config.base_url };
            info!(model = %config.model, base_url = base, "llm backend: api_key");
        }
    }

    // `albert login` — interactive ChatGPT-subscription sign-in, then exit (no
    // runtime, no memory vault needed). Writes tokens to the subscription store.
    if std::env::args().nth(1).as_deref() == Some("login") {
        info!(path = %config.subscription_auth_json.display(), "login: writing subscription tokens");
        return openai_login::run(&config.subscription_auth_json).await;
    }

    // ── Code workspace: the jail octo-code's file tools operate in ───────────
    // Exported so the tools (which read OCTO_CODE_WORKSPACE at call time) and the
    // storage/telegram workspace bridge all agree on one directory.
    let _ = create_dir_all(&config.code_workspace);
    set_var(WORKSPACE_ENV, &config.code_workspace);
    // The skills root, exported the same way: forkd runs a skill's bundled scripts
    // in place from here (skill_path) — one source of truth, by name.
    set_var(SKILLS_ENV, &config.skills_dir);
    info!(workspace = %config.code_workspace.display(), skills = %config.skills_dir.display(), "code workspace + skills root exported");

    // ── Memory: kaeru, scoped to the "albert" initiative ─────────────────────
    // Local-only by default; if albert.toml declares [clouds.*], build a
    // CloudRegistry (endpoint URL + bearer from the named env var) and hand it to
    // kaeru so the share/pull/cloud_recall tools come alive. Which tools get
    // installed is decided per-turn by the same emptiness check (cogitator::drive).
    let kcfg = KaeruConfig::from_env().map_err(|e| Error::Kaeru(e.to_string()))?;
    let store = Arc::new(Store::open_with_config(kcfg).map_err(|e| Error::Kaeru(e.to_string()))?);
    let memory = if config.clouds.is_empty() {
        info!("memory: kaeru (initiative=albert, local-only)");
        KaeruMemory::with_initiative(store, "albert")
    } else {
        let clients: HashMap<String, CloudClient> = config
            .clouds
            .iter()
            .map(|(name, ep)| {
                let token = var(&ep.token_env).unwrap_or_default();
                if token.is_empty() {
                    warn!(cloud = %name, env = %ep.token_env, "cloud token env is unset");
                }
                (name.clone(), CloudClient::new(ep.url.clone(), token))
            })
            .collect();
        let names: Vec<&str> = config.clouds.keys().map(String::as_str).collect();
        info!(clouds = %names.join(", "), "memory: kaeru (initiative=albert) + clouds");
        let registry = CloudRegistry::new(clients, config.clouds_default.clone());
        KaeruMemory::with_clouds(store, "albert", registry)
    };

    // ── Hot context: per-channel transcript backend ──────────────────────────
    const HISTORY_MAX: usize = 30;
    let history: Arc<dyn HistoryStore> = match config.history.as_deref() {
        Some(spec) if spec.starts_with("sqlite:") => {
            let path = &spec["sqlite:".len()..];
            info!(path, "history: sqlite backend (migrated, persistent)");
            Arc::new(SqliteHistory::open(path, HISTORY_MAX).await?)
        }
        Some(spec) if spec.starts_with("file:") => {
            let dir = &spec["file:".len()..];
            info!(dir, "history: file backend");
            Arc::new(FileHistory::new(dir, HISTORY_MAX)?)
        }
        _ => {
            info!("history: in-memory backend");
            Arc::new(InMemoryHistory::new(HISTORY_MAX))
        }
    };

    // ── Scheduler connector (cron/reminders) ─────────────────────────────────
    let scheduler = Scheduler::new("scheduler", config.scheduler_state_path.clone());
    info!(state = %config.scheduler_state_path.display(), "scheduler connector");

    // ── Persona + instructions (RAM, hot-reloaded) ───────────────────────────
    let prompt = PromptFiles::new(config.soul_path.clone(), config.system_path.clone());
    info!(
        soul = %config.soul_path.display(),
        system = %config.system_path.display(),
        "prompt files (hot-reloaded)"
    );

    // ── Loop scratchpad: super-operational per-task working object ───────────
    let scratchpad = ScratchpadStore::new();

    // ── Declarative skills: a folder Albert lists + applies (LRU-cached) ─────
    // What this runtime can actually do gates which skills exist: transcription rides
    // on the ChatGPT subscription token, so under an API key that skill is not merely
    // unusable, it's absent (a skill's `requires:` is matched against this list).
    let mut capabilities: Vec<&str> = Vec::new();
    if config.auth == AuthMode::Subscription {
        capabilities.push("subscription");
    }
    let skills = SkillStore::load(config.skills_dir.clone(), config.skills_cache, config.skills_page, &capabilities);
    info!(dir = %config.skills_dir.display(), cache = config.skills_cache, page = config.skills_page, "skills store");

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
        info!(manifest = %config.connectors_manifest.display(), "channels: telegram (ACL) + calendar + storage + forkd + search + http (+ mail if a manifest is present)");
        // The mail factory is registered so the organ CAN be enabled, but no
        // config/connectors/mail manifest ships by default (only mail.toml.example),
        // so from_config_file instantiates it only once a real manifest is dropped in.
        builder = builder
            .register_connector_type("telegram", telegram_factory())
            .register_connector_type("caldav", caldav_factory())
            .register_connector_type("storage", storage_factory())
            .register_connector_type("forkd", forkd_factory())
            .register_connector_type("search", search_factory())
            .register_connector_type("http", http_factory())
            .register_connector_type("mail", mail_factory())
            .from_config_file(&config.connectors_manifest)?;
    } else {
        info!("channel: console (set OCTO_TELEGRAM_TOKEN for telegram + calendar)");
        builder = builder.add_connector(ConsoleConnector::new("console"));
    }

    builder.build().run().await?;
    Ok(())
}
