//! Albert config, loaded from a TOML file (config-as-data, like Octo's connector
//! manifests). The file's path comes from `ALBERT_CONFIG` (default: next to the
//! binary, else the working directory). Secrets stay in the environment, named in the
//! TOML by their env-var name — so the file is safe to commit.
//!
//! This is *Albert-level* config (model, prompts, memory reflection, history).
//! The connectors (Telegram, calendar) are config-driven separately, through
//! Octo's `from_config_file` + per-connector manifests under `config/connectors/`.
//!
//! Relative paths in the TOML (prompt files, state files) resolve against the
//! config file's own directory.

use std::{collections::HashMap, env::current_exe, env::var, path::Path, path::PathBuf};

use octo_approval::AuthoritySettings;
use serde::{Deserialize, Serialize};

use crate::{
    context::Settings as ContextSettings, memory::config::MemoryConfig, models::PoolConfig,
    scratchpad::Settings as ScratchpadSettings, sessions::settings::SessionSettings,
    subagents::Limits,
};

mod control_base;
mod load;
mod schema;
pub use self::schema::{d_max_turns, d_reflect, d_skills_cache, d_skills_page};
pub mod history;
mod secrets;

/// How Albert authenticates to the model backend.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    /// An API key against OpenRouter or any OpenAI-compatible endpoint. The default.
    ApiKey,
    /// A ChatGPT **subscription**: OAuth tokens routed to the Codex backend.
    Subscription,
}

/// One kaeru-cloud endpoint Albert can share to / pull from: its URL and the
/// env-var **name** holding its bearer token (the value stays in the environment,
/// never in the committed config).
#[derive(Clone, Debug, Serialize)]
pub struct CloudEndpoint {
    pub url: String,
    pub token_env: String,
}

/// The resolved config the rest of the crate uses (secret already pulled from env).
#[derive(Clone)]
pub struct Config {
    pub sessions: SessionSettings,
    pub authorization: AuthoritySettings,
    pub context: ContextSettings,
    pub subagents: Limits,
    pub scratchpad: ScratchpadSettings,
    pub model_pool: Option<PathBuf>,
    pub mcp_registry: PathBuf,
    pub models: Option<PoolConfig>,
    pub memory: MemoryConfig,
    pub(crate) memory_explicit: bool,
    pub model: String,
    /// How the LLM call authenticates — API key (default) or ChatGPT subscription.
    pub auth: AuthMode,
    pub base_url: String,
    /// The LLM API key — present only in [`AuthMode::ApiKey`]; `None` under a
    /// subscription (where the bearer comes from the OAuth token store instead).
    pub api_key: Option<String>,
    pub openai_key_env: String,
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
    pub history_backup_dir: PathBuf,
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
    /// Page size for `skill_list` only. The preamble always shows every available
    /// skill's name and description, independently of this setting.
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
/// `albert.toml` in the working directory.
pub fn config_path() -> PathBuf {
    if let Ok(path) = var("ALBERT_CONFIG") {
        return PathBuf::from(path);
    }
    if let Ok(exe) = current_exe() {
        if let Some(dir) = exe.parent() {
            let beside = dir.join("albert.toml");
            if beside.exists() {
                return beside;
            }
        }
    }
    PathBuf::from("albert.toml")
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
        assert!(default_multimodal(
            AuthMode::ApiKey,
            "google/gemini-2.5-pro"
        ));
        assert!(default_multimodal(
            AuthMode::ApiKey,
            "qwen/qwen2.5-vl-72b-instruct"
        ));
        assert!(!default_multimodal(
            AuthMode::ApiKey,
            "deepseek/deepseek-chat"
        ));
    }
}
