//! Albert — the assembly. Octo runtime + an LLM cogitator with kaeru memory and a
//! scheduler connector. Config lives in a TOML file (path via `ALBERT_CONFIG`);
//! secrets stay in `.env`. Talk to it (Telegram if a token is set, else console);
//! it remembers and it reminds.

mod acl;
mod codex_http;
mod codex_model;
mod cogitator;
mod commands;
mod config;
mod connector_catalog;
mod console;
mod error;
mod history;
mod manifests;
mod memory;
mod models;
mod openai_login;
mod openrouter_http;
mod prompt;
mod routines;
mod scratchpad;
mod selfconfig;
mod skills;
mod status;
mod subagents;

use std::{
    env::{set_var, var},
    fs::create_dir_all,
    sync::Arc,
};

use dotenvy::{dotenv, from_path};
use octo_code::WORKSPACE_ENV;
use octo_connector_browser::factory as browser_factory;
use octo_connector_caldav::factory as caldav_factory;
use octo_connector_forkd::{factory as forkd_factory, SKILLS_ENV};
use octo_connector_http::factory as http_factory;
use octo_connector_imagegen::factory as imagegen_factory;
use octo_connector_mail::{ensure_crypto_provider, factory as mail_factory};
use octo_connector_scheduler::Scheduler;
use octo_connector_search::factory as search_factory;
use octo_connector_speak::factory as speak_factory;
use octo_connector_storage::factory as storage_factory;
use octo_connector_telegram::factory as telegram_factory;
use octo_connector_transcribe::factory as transcribe_factory;
use octo_core::Octo;
use octo_openai_auth::SubscriptionAuth;
use tracing::info;
use tracing_subscriber::{fmt, EnvFilter};

use crate::{
    cogitator::AlbertCogitator,
    config::{AuthMode, Config},
    console::ConsoleConnector,
    error::Result,
    history::{FileHistory, HistoryStore, InMemoryHistory, SqliteHistory},
    manifests::declared_types,
    memory::Memory,
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
             octo_connector_http=info,octo_connector_browser=info,\
             octo_connector_transcribe=info,octo_connector_speak=info,\
             octo_connector_imagegen=info,octo_core=warn"
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
    if let Some(pool) = &config.models {
        info!(preferred = %pool.default, members = pool.models.len(), max_attempts = pool.max_attempts, "llm model pool loaded");
    } else {
        match config.auth {
            AuthMode::Subscription => {
                info!(model = %config.model, base_url = %config.subscription_base_url, "llm backend: subscription");
            }
            AuthMode::ApiKey => {
                let base = if config.base_url.is_empty() {
                    "(provider default)"
                } else {
                    &config.base_url
                };
                info!(model = %config.model, base_url = base, "llm backend: api_key");
            }
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

    // Connect and migrate memory before accepting any events.
    let memory = Memory::open(&config).await?;

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
    // Calendar-style (`cron`) alarms fall back to the owner's timezone.
    let scheduler = Scheduler::with_timezone(
        "scheduler",
        config.scheduler_state_path.clone(),
        config.timezone.name(),
    );
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
    // What this runtime can actually do gates which skills exist: a skill that needs,
    // say, image generation is not merely unusable without it, it's absent (a skill's
    // `requires:` is matched against this list).
    // Connector-backed capabilities follow the manifests (the octo loader instantiates
    // them only in the telegram setup, and the subscription organs only with a token).
    let has_telegram = var("OCTO_TELEGRAM_TOKEN")
        .map(|t| !t.trim().is_empty())
        .unwrap_or(false);
    let declared = if has_telegram {
        declared_types(&config.connectors_manifest)
    } else {
        Default::default()
    };
    let token = config.subscription_auth_json.exists();
    let mut capabilities: Vec<&str> = Vec::new();
    if config.auth == AuthMode::Subscription {
        capabilities.push("subscription");
    }
    if token && declared.contains("imagegen") {
        capabilities.push("imagegen");
    }
    let skills = SkillStore::load(
        config.skills_dir.clone(),
        config.skills_cache,
        config.skills_page,
        &capabilities,
    );
    info!(dir = %config.skills_dir.display(), cache = config.skills_cache, page = config.skills_page, "skills store");

    // Shared, refresh-serialised subscription auth — ONE refresh owner across the LLM
    // backend and the subscription organs (transcribe, speak, imagegen). Its errors point at
    // Albert's own sign-in command.
    let auth = Arc::new(
        SubscriptionAuth::new(config.subscription_auth_json.clone())
            .with_login_hint("albert login"),
    );

    let mut builder = Octo::builder()
        .cogitator(AlbertCogitator::new(
            "albert",
            config.clone(),
            history,
            memory,
            scratchpad,
            skills,
            prompt,
            auth.clone(),
        ))
        .add_connector(scheduler);

    // ── Connectors: config-driven Telegram (ACL) + calendar, or console ──────
    // With a token present, the Telegram channel and the calendar are assembled
    // from config/connectors/*/*.toml via their factories (secrets stay in env,
    // named in each manifest; owner_chat + the ACL live in telegram's manifest).
    // Otherwise a console channel (no calendar in that dev mode).
    if has_telegram {
        info!(manifest = %config.connectors_manifest.display(), "channels: telegram (ACL) + calendar + storage + forkd + search + browser + http (+ mail if a manifest is present)");
        // The mail factory is registered so the organ CAN be enabled, but no
        // config/connectors/mail manifest ships by default (only mail.toml.example),
        // so from_config_file instantiates it only once a real manifest is dropped in.
        builder = builder
            .register_connector_type("telegram", telegram_factory())
            .register_connector_type("caldav", caldav_factory())
            .register_connector_type("storage", storage_factory())
            .register_connector_type("forkd", forkd_factory())
            .register_connector_type("search", search_factory())
            .register_connector_type("browser", browser_factory())
            .register_connector_type("http", http_factory())
            .register_connector_type("mail", mail_factory())
            // Subscription organs (voice in/out, image synthesis): each is on while its
            // manifest is present, with its settings in that manifest. Every factory shares
            // the cogitator's `auth` (one token owner) and skips itself without a token, so
            // `auth = "api_key"` for the LLM and a subscription auth.json for these combine.
            .register_connector_type("transcribe", transcribe_factory(auth.clone()))
            .register_connector_type("speak", speak_factory(auth.clone()))
            .register_connector_type("imagegen", imagegen_factory(auth.clone()))
            .from_config_file(&config.connectors_manifest)?;
        let subscription: Vec<&str> = ["transcribe", "speak", "imagegen"]
            .into_iter()
            .filter(|t| declared.contains(*t))
            .collect();
        if !subscription.is_empty() {
            info!(
                organs = %subscription.join(", "),
                token = token,
                auth_json = %config.subscription_auth_json.display(),
                "subscription organs declared"
            );
        }
    } else {
        info!("channel: console (set OCTO_TELEGRAM_TOKEN for telegram + calendar)");
        builder = builder.add_connector(ConsoleConnector::new("console"));
    }

    builder.build().run().await?;
    Ok(())
}
