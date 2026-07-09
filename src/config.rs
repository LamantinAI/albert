//! Albert config, loaded from a TOML file (config-as-data, like Octo's connector
//! manifests). The file's path comes from `ALBERT_CONFIG` (default: next to the
//! binary, else this crate dir). Secrets stay in the environment, named in the
//! TOML by their env-var name — so the file is safe to commit.
//!
//! This is *Albert-level* config (model, prompts, memory reflection, history).
//! The connectors (Telegram, calendar) are config-driven separately, through
//! Octo's `from_config_file` + per-connector manifests under `config/connectors/`.
//!
//! Relative paths in the TOML (prompt files, state files) resolve against the
//! config file's own directory.

use std::{
    env::{current_exe, var},
    fs::read_to_string,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use toml::from_str;

use crate::error::{Error, Result};

/// The resolved config the rest of the crate uses (secret already pulled from env).
#[derive(Clone)]
pub struct Config {
    pub model: String,
    pub base_url: String,
    pub api_key: String,
    pub history: Option<String>,
    pub scheduler_state_path: PathBuf,
    pub reflection_secs: u64,
    pub soul_path: PathBuf,
    pub system_path: PathBuf,
    pub max_tool_turns: usize,
    /// Octo connector-manifest root (`from_config_file`). Resolved against the
    /// config dir so a relocated deployment finds it (default `config/octo.toml`).
    pub connectors_manifest: PathBuf,
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = config_path();
        let text = read_to_string(&path)
            .map_err(|e| Error::Config(format!("{}: {e}", path.display())))?;
        let raw: Raw = from_str(&text)?;
        let dir = path.parent().unwrap_or_else(|| Path::new("."));

        let key_var = raw.openai_key_env.as_deref().unwrap_or("ALBERT_OPENAI_KEY");
        let api_key = var(key_var).map_err(|_| Error::Config(format!("missing secret env {key_var}")))?;

        Ok(Config {
            model: raw.model,
            base_url: raw.base_url,
            api_key,
            history: Some(raw.history.backend),
            scheduler_state_path: resolve(dir, &raw.scheduler.state_path),
            reflection_secs: raw.reflection.period_secs,
            soul_path: resolve(dir, &raw.prompt.soul),
            system_path: resolve(dir, &raw.prompt.system),
            max_tool_turns: raw.agent.max_tool_turns,
            connectors_manifest: resolve(
                dir,
                raw.connectors_manifest.as_deref().unwrap_or("config/octo.toml"),
            ),
        })
    }
}

/// Locate the config: `ALBERT_CONFIG`, else `albert.toml` next to the binary, else
/// this crate's `albert.toml`.
fn config_path() -> PathBuf {
    if let Ok(p) = var("ALBERT_CONFIG") {
        return PathBuf::from(p);
    }
    if let Ok(exe) = current_exe() {
        if let Some(dir) = exe.parent() {
            let beside = dir.join("albert.toml");
            if beside.exists() {
                return beside;
            }
        }
    }
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/albert.toml"))
}

/// A relative path in the TOML resolves against the config file's directory.
fn resolve(dir: &Path, p: &str) -> PathBuf {
    let pb = PathBuf::from(p);
    if pb.is_absolute() {
        pb
    } else {
        dir.join(pb)
    }
}

// ── Raw TOML shape (defaults let any table/field be omitted) ─────────────────

#[derive(Deserialize)]
struct Raw {
    model: String,
    #[serde(default)]
    base_url: String,
    #[serde(default)]
    openai_key_env: Option<String>,
    #[serde(default)]
    connectors_manifest: Option<String>,
    #[serde(default)]
    prompt: RawPrompt,
    #[serde(default)]
    reflection: RawReflection,
    #[serde(default)]
    history: RawHistory,
    #[serde(default)]
    scheduler: RawScheduler,
    #[serde(default)]
    agent: RawAgent,
}

#[derive(Deserialize)]
struct RawPrompt {
    #[serde(default = "d_soul")]
    soul: String,
    #[serde(default = "d_system")]
    system: String,
}
impl Default for RawPrompt {
    fn default() -> Self {
        Self { soul: d_soul(), system: d_system() }
    }
}

#[derive(Deserialize)]
struct RawReflection {
    #[serde(default = "d_reflect")]
    period_secs: u64,
}
impl Default for RawReflection {
    fn default() -> Self {
        Self { period_secs: d_reflect() }
    }
}

#[derive(Deserialize)]
struct RawHistory {
    #[serde(default = "d_backend")]
    backend: String,
}
impl Default for RawHistory {
    fn default() -> Self {
        Self { backend: d_backend() }
    }
}

#[derive(Deserialize)]
struct RawScheduler {
    #[serde(default = "d_state")]
    state_path: String,
}
impl Default for RawScheduler {
    fn default() -> Self {
        Self { state_path: d_state() }
    }
}

#[derive(Deserialize)]
struct RawAgent {
    #[serde(default = "d_max_turns")]
    max_tool_turns: usize,
}
impl Default for RawAgent {
    fn default() -> Self {
        Self { max_tool_turns: d_max_turns() }
    }
}

fn d_soul() -> String {
    "soul.md".into()
}
fn d_system() -> String {
    "system.md".into()
}
fn d_reflect() -> u64 {
    21_600
}
fn d_backend() -> String {
    "memory".into()
}
fn d_state() -> String {
    "state/scheduler.json".into()
}
fn d_max_turns() -> usize {
    8
}
