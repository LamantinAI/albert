//! Deserialization schema and defaults for the agent configuration.
use super::history::default_backup_dir;
use crate::{
    context::Settings as ContextSettings, memory::config::MemoryConfig,
    scratchpad::Settings as ScratchpadSettings, sessions::settings::SessionSettings,
    subagents::Limits,
};
use octo_approval::AuthoritySettings;
use serde::Deserialize;
use toml::Table;
// ── Raw TOML shape (defaults let any table/field be omitted) ─────────────────

#[derive(Deserialize)]
pub(super) struct Raw {
    #[serde(default)]
    pub(super) sessions: SessionSettings,
    #[serde(default)]
    pub(super) authorization: AuthoritySettings,
    #[serde(default)]
    pub(super) context: ContextSettings,
    #[serde(default)]
    pub(super) subagents: Limits,
    #[serde(default)]
    pub(super) scratchpad: ScratchpadSettings,
    #[serde(default)]
    pub(super) model_pool: Option<String>,
    pub(super) mcp_registry: Option<String>,
    #[serde(default)]
    pub(super) memory: MemoryConfig,
    #[serde(default)]
    pub(super) model: String,
    /// `"api_key"` (default) or `"subscription"`.
    #[serde(default)]
    pub(super) auth: Option<String>,
    /// Force vision on/off; absent → the [`default_multimodal`] heuristic.
    #[serde(default)]
    pub(super) multimodal: Option<bool>,

    /// Stream live turn progress (tool calls / thoughts) into the chat.
    #[serde(default)]
    pub(super) stream_status: Option<bool>,
    #[serde(default)]
    pub(super) base_url: String,
    #[serde(default)]
    pub(super) openai_key_env: Option<String>,
    #[serde(default)]
    pub(super) subscription: RawSubscription,
    #[serde(default)]
    pub(super) connectors_manifest: Option<String>,
    #[serde(default)]
    pub(super) timezone: Option<String>,
    #[serde(default)]
    pub(super) prompt: RawPrompt,
    #[serde(default)]
    pub(super) reflection: RawReflection,
    #[serde(default)]
    pub(super) history: RawHistory,
    #[serde(default)]
    pub(super) scheduler: RawScheduler,
    #[serde(default)]
    pub(super) agent: RawAgent,
    #[serde(default)]
    pub(super) skills: RawSkills,
    #[serde(default)]
    pub(super) clouds: Table,
    #[serde(default)]
    pub(super) code: RawCode,
}

#[derive(Deserialize)]
pub(super) struct RawSubscription {
    #[serde(default = "d_auth_json")]
    pub(super) auth_json: String,
    #[serde(default = "d_codex_base")]
    pub(super) base_url: String,
}
impl Default for RawSubscription {
    fn default() -> Self {
        Self {
            auth_json: d_auth_json(),
            base_url: d_codex_base(),
        }
    }
}

#[derive(Deserialize)]
pub(super) struct RawPrompt {
    #[serde(default = "d_soul")]
    pub(super) soul: String,
    #[serde(default = "d_system")]
    pub(super) system: String,
}
impl Default for RawPrompt {
    fn default() -> Self {
        Self {
            soul: d_soul(),
            system: d_system(),
        }
    }
}

#[derive(Deserialize)]
pub(super) struct RawReflection {
    #[serde(default = "d_reflect")]
    pub(super) period_secs: u64,
}
impl Default for RawReflection {
    fn default() -> Self {
        Self {
            period_secs: d_reflect(),
        }
    }
}

#[derive(Deserialize)]
pub(super) struct RawHistory {
    #[serde(default = "d_backend")]
    pub(super) backend: String,
    #[serde(default = "default_backup_dir")]
    pub(super) backup_dir: String,
}
impl Default for RawHistory {
    fn default() -> Self {
        Self {
            backend: d_backend(),
            backup_dir: default_backup_dir(),
        }
    }
}

#[derive(Deserialize)]
pub(super) struct RawScheduler {
    #[serde(default = "d_state")]
    pub(super) state_path: String,
}
impl Default for RawScheduler {
    fn default() -> Self {
        Self {
            state_path: d_state(),
        }
    }
}

#[derive(Deserialize)]
pub(super) struct RawAgent {
    #[serde(default = "d_max_turns")]
    pub(super) max_tool_turns: usize,
}
impl Default for RawAgent {
    fn default() -> Self {
        Self {
            max_tool_turns: d_max_turns(),
        }
    }
}

#[derive(Deserialize)]
pub(super) struct RawSkills {
    #[serde(default = "d_skills_dir")]
    pub(super) dir: String,
    #[serde(default = "d_skills_cache")]
    pub(super) cache: usize,
    #[serde(default = "d_skills_page")]
    pub(super) page: usize,
}
impl Default for RawSkills {
    fn default() -> Self {
        Self {
            dir: d_skills_dir(),
            cache: d_skills_cache(),
            page: d_skills_page(),
        }
    }
}

#[derive(Deserialize)]
pub(super) struct RawCode {
    #[serde(default = "d_code_workspace")]
    pub(super) workspace: String,
}
impl Default for RawCode {
    fn default() -> Self {
        Self {
            workspace: d_code_workspace(),
        }
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
pub fn d_reflect() -> u64 {
    21_600
}
fn d_backend() -> String {
    "sqlite:state/history.db".into()
}
fn d_state() -> String {
    "state/scheduler.json".into()
}
pub fn d_max_turns() -> usize {
    8
}
fn d_skills_dir() -> String {
    "skills".into()
}
pub fn d_skills_cache() -> usize {
    5
}
pub fn d_skills_page() -> usize {
    10
}
fn d_code_workspace() -> String {
    "state/workspace".into()
}
