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
    collections::HashMap,
    env::{current_exe, var},
    fs::read_to_string,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use toml::{from_str, Table, Value};

use crate::error::{Error, Result};

/// How Albert authenticates to the model backend.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuthMode {
    /// An API key against OpenRouter or any OpenAI-compatible endpoint. The default.
    ApiKey,
    /// A ChatGPT **subscription**: OAuth tokens routed to the Codex backend.
    Subscription,
}

/// One kaeru-cloud endpoint Albert can share to / pull from: its URL and the
/// env-var **name** holding its bearer token (the value stays in the environment,
/// never in the committed config).
#[derive(Clone, Debug)]
pub struct CloudEndpoint {
    pub url: String,
    pub token_env: String,
}

/// The resolved config the rest of the crate uses (secret already pulled from env).
#[derive(Clone)]
pub struct Config {
    pub model: String,
    /// How the LLM call authenticates — API key (default) or ChatGPT subscription.
    pub auth: AuthMode,
    pub base_url: String,
    /// The LLM API key — present only in [`AuthMode::ApiKey`]; `None` under a
    /// subscription (where the bearer comes from the OAuth token store instead).
    pub api_key: Option<String>,
    /// Subscription mode: the codex-style `auth.json` to read tokens from, and the
    /// Codex backend base URL model calls hit (`.../responses` is appended by rig).
    pub subscription_auth_json: PathBuf,
    pub subscription_base_url: String,
    /// Whether the configured model can see images — gates photo perception.
    /// Explicit `multimodal = true/false` in the TOML wins; the default is a
    /// heuristic: a ChatGPT subscription is always vision-capable, an API-key
    /// model is matched by name against the known multimodal families.
    pub multimodal: bool,
    /// Stream the agent's live progress (tool calls, reasoning summaries) into
    /// the chat as `chat.status` envelopes while a turn runs. Default: on.
    pub stream_status: bool,
    pub history: Option<String>,
    pub scheduler_state_path: PathBuf,
    pub reflection_secs: u64,
    pub soul_path: PathBuf,
    pub system_path: PathBuf,
    pub max_tool_turns: usize,
    /// Octo connector-manifest root (`from_config_file`). Resolved against the
    /// config dir so a relocated deployment finds it (default `config/octo.toml`).
    pub connectors_manifest: PathBuf,
    /// Declarative skills folder (`skills/<name>/SKILL.md`), resolved against the
    /// config dir. Optional — a missing folder just means no skills.
    pub skills_dir: PathBuf,
    /// How many applied skill bodies stay hot in RAM (LRU); the rest are listed but
    /// loaded on demand.
    pub skills_cache: usize,
    /// Page size for browsing the skill catalog (`skill_list`), and — for a large
    /// library — the same threshold above which the per-turn catalog collapses to a
    /// count + `skill_search` pointer instead of the full list.
    pub skills_page: usize,
    /// The owner's timezone (IANA name). Used to render the "current time" the
    /// agent reasons from, so "today", reminders, and shown times are local
    /// rather than UTC. Defaults to UTC.
    pub timezone: chrono_tz::Tz,
    /// kaeru-cloud endpoints (name -> endpoint) Albert can share to / pull from.
    /// Empty when no `[clouds.*]` is configured — Albert stays local-only.
    pub clouds: HashMap<String, CloudEndpoint>,
    /// The default cloud name (`[clouds] default`), if set.
    pub clouds_default: Option<String>,
    /// Ephemeral workspace root for octo-code file tools (`$OCTO_CODE_WORKSPACE`),
    /// resolved against the config dir. octo-code jails all file ops to it.
    pub code_workspace: PathBuf,
    /// The deployment directory — the folder holding `albert.toml`, `soul.md`,
    /// `system.md`, `config/`, `skills/`. The owner-only self-config tools jail their
    /// (allow-listed) reads/writes to it, so Albert can edit its own configuration.
    pub deploy_dir: PathBuf,
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = config_path();
        let text = read_to_string(&path)
            .map_err(|e| Error::Config(format!("{}: {e}", path.display())))?;
        let raw: Raw = from_str(&text)?;
        let dir = path.parent().unwrap_or_else(|| Path::new("."));

        let key_var = raw.openai_key_env.as_deref().unwrap_or("ALBERT_OPENAI_KEY");
        let auth = match raw.auth.as_deref() {
            None | Some("api_key") => AuthMode::ApiKey,
            Some("subscription") => AuthMode::Subscription,
            Some(other) => {
                return Err(Error::Config(format!(
                    "unknown auth {other:?}; use \"api_key\" or \"subscription\""
                )))
            }
        };
        // The API key is required only for the api-key path; a subscription reads
        // its bearer from the OAuth token store, so the env var may be unset.
        let api_key = match auth {
            AuthMode::ApiKey => {
                Some(var(key_var).map_err(|_| Error::Config(format!("missing secret env {key_var}")))?)
            }
            AuthMode::Subscription => var(key_var).ok(),
        };

        let timezone = raw
            .timezone
            .as_deref()
            .map(str::parse::<chrono_tz::Tz>)
            .transpose()
            .map_err(|e| Error::Config(format!("invalid timezone: {e}")))?
            .unwrap_or(chrono_tz::UTC);

        let multimodal = raw
            .multimodal
            .unwrap_or_else(|| default_multimodal(auth, &raw.model));

        // Cloud memory: one [clouds.<name>] table each (url + token_env), plus an
        // optional [clouds] default. Absent -> empty map -> Albert stays local-only.
        let mut clouds = HashMap::new();
        let mut clouds_default = None;
        for (name, val) in &raw.clouds {
            if name == "default" {
                clouds_default = val.as_str().map(str::to_string);
                continue;
            }
            match (
                val.get("url").and_then(Value::as_str),
                val.get("token_env").and_then(Value::as_str),
            ) {
                (Some(url), Some(token_env)) => {
                    clouds.insert(
                        name.clone(),
                        CloudEndpoint { url: url.to_string(), token_env: token_env.to_string() },
                    );
                }
                _ => {
                    return Err(Error::Config(format!(
                        "cloud '{name}' needs both url and token_env"
                    )))
                }
            }
        }

        Ok(Config {
            multimodal,
            stream_status: raw.stream_status.unwrap_or(true),
            model: raw.model,
            auth,
            base_url: raw.base_url,
            api_key,
            subscription_auth_json: resolve_path(dir, &raw.subscription.auth_json),
            subscription_base_url: raw.subscription.base_url,
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
            skills_dir: resolve(dir, &raw.skills.dir),
            skills_cache: raw.skills.cache,
            skills_page: raw.skills.page.max(1),
            timezone,
            clouds,
            clouds_default,
            code_workspace: resolve(dir, &raw.code.workspace),
            deploy_dir: dir.to_path_buf(),
        })
    }
}

/// Vision heuristic for when the TOML doesn't say: every ChatGPT-subscription
/// (Codex) model is multimodal; an API-key (OpenRouter et al.) model is matched
/// by name against the known multimodal families. Wrong guess → set
/// `multimodal = true/false` explicitly.
fn default_multimodal(auth: AuthMode, model: &str) -> bool {
    if auth == AuthMode::Subscription {
        return true;
    }
    const VISION_HINTS: &[&str] = &[
        "gpt-4o", "gpt-4.1", "gpt-5", "o3", "o4", "chatgpt", "claude", "gemini", "pixtral",
        "llava", "vision", "-vl", "qwen-vl", "llama-4", "grok-4", "grok-3",
    ];
    let m = model.to_lowercase();
    VISION_HINTS.iter().any(|h| m.contains(h))
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

/// Like [`resolve`], but first expands a leading `~/` (or a bare `~`) to `$HOME` —
/// so a subscription token path like `~/.codex/auth.json` lands in the real home
/// directory rather than under the config dir.
fn resolve_path(dir: &Path, p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Ok(home) = var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    if p == "~" {
        if let Ok(home) = var("HOME") {
            return PathBuf::from(home);
        }
    }
    resolve(dir, p)
}

// ── Raw TOML shape (defaults let any table/field be omitted) ─────────────────

#[derive(Deserialize)]
struct Raw {
    model: String,
    /// `"api_key"` (default) or `"subscription"`.
    #[serde(default)]
    auth: Option<String>,
    /// Force vision on/off; absent → the [`default_multimodal`] heuristic.
    #[serde(default)]
    multimodal: Option<bool>,
    /// Stream live turn progress (tool calls / thoughts) into the chat.
    #[serde(default)]
    stream_status: Option<bool>,
    #[serde(default)]
    base_url: String,
    #[serde(default)]
    openai_key_env: Option<String>,
    #[serde(default)]
    subscription: RawSubscription,
    #[serde(default)]
    connectors_manifest: Option<String>,
    #[serde(default)]
    timezone: Option<String>,
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
    #[serde(default)]
    skills: RawSkills,
    #[serde(default)]
    clouds: Table,
    #[serde(default)]
    code: RawCode,
}

#[derive(Deserialize)]
struct RawSubscription {
    #[serde(default = "d_auth_json")]
    auth_json: String,
    #[serde(default = "d_codex_base")]
    base_url: String,
}
impl Default for RawSubscription {
    fn default() -> Self {
        Self { auth_json: d_auth_json(), base_url: d_codex_base() }
    }
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

#[derive(Deserialize)]
struct RawSkills {
    #[serde(default = "d_skills_dir")]
    dir: String,
    #[serde(default = "d_skills_cache")]
    cache: usize,
    #[serde(default = "d_skills_page")]
    page: usize,
}
impl Default for RawSkills {
    fn default() -> Self {
        Self { dir: d_skills_dir(), cache: d_skills_cache(), page: d_skills_page() }
    }
}

#[derive(Deserialize)]
struct RawCode {
    #[serde(default = "d_code_workspace")]
    workspace: String,
}
impl Default for RawCode {
    fn default() -> Self {
        Self { workspace: d_code_workspace() }
    }
}

fn d_auth_json() -> String {
    "~/.codex/auth.json".into()
}
fn d_codex_base() -> String {
    "https://chatgpt.com/backend-api/codex".into()
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
fn d_skills_dir() -> String {
    "skills".into()
}
fn d_skills_cache() -> usize {
    5
}
fn d_skills_page() -> usize {
    10
}
fn d_code_workspace() -> String {
    "state/workspace".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscription_is_always_multimodal() {
        assert!(default_multimodal(AuthMode::Subscription, "whatever"));
    }

    #[test]
    fn api_key_models_match_by_name() {
        assert!(default_multimodal(AuthMode::ApiKey, "openai/gpt-4o"));
        assert!(default_multimodal(AuthMode::ApiKey, "google/gemini-2.5-pro"));
        assert!(default_multimodal(AuthMode::ApiKey, "qwen/qwen2.5-vl-72b-instruct"));
        assert!(!default_multimodal(AuthMode::ApiKey, "deepseek/deepseek-chat"));
    }
}
