use chrono_tz::UTC;
use serde_json::{json, Value};

use crate::{
    api::models::{ConfigField, ConfigFieldKind},
    config::{d_max_turns, d_reflect, d_skills_cache, d_skills_page},
    context::Settings as ContextSettings,
    sessions::settings::SessionSettings,
    subagents::Limits,
};

fn field(
    path: &str,
    group: &str,
    label: &str,
    description: &str,
    default_value: Value,
) -> ConfigField {
    let kind = if default_value.is_boolean() {
        ConfigFieldKind::Boolean
    } else if default_value.is_i64() {
        ConfigFieldKind::Integer
    } else {
        ConfigFieldKind::Text
    };
    let numeric = matches!(kind, ConfigFieldKind::Integer);
    ConfigField {
        path: path.into(),
        group: group.into(),
        label: label.into(),
        description: description.into(),
        kind,
        value: None,
        default_value,
        minimum: numeric.then_some(
            if matches!(
                path,
                "skills.cache" | "skills.page" | "agent.max_tool_turns"
            ) {
                0
            } else {
                1
            },
        ),
        maximum: numeric.then_some(if path == "context.new_messages_percent" {
            99
        } else {
            9_007_199_254_740_991
        }),
    }
}

pub(super) fn fields() -> Vec<ConfigField> {
    let context = ContextSettings::default();
    let sessions = SessionSettings::default();
    let children = Limits::default();
    vec![
        field(
            "timezone",
            "General",
            "Timezone",
            "IANA timezone for dates, reminders and current time.",
            json!(UTC.name()),
        ),
        field(
            "stream_status",
            "General",
            "Live progress",
            "Show tool activity while Albert works.",
            json!(true),
        ),
        field(
            "agent.max_tool_turns",
            "General",
            "Tool rounds per turn",
            "Maximum tool rounds for a main-agent turn.",
            json!(d_max_turns()),
        ),
        field(
            "sessions.titles.enabled",
            "Conversations",
            "Automatic titles",
            "Name new conversations from their content.",
            json!(sessions.titles.enabled),
        ),
        field(
            "sessions.limits.temporary_max_sessions",
            "Conversations",
            "Temporary conversations",
            "Maximum number of retained temporary conversations.",
            json!(sessions.limits.temporary_max_sessions),
        ),
        field(
            "sessions.limits.temporary_max_turns",
            "Conversations",
            "Temporary turn limit",
            "Maximum turns in one temporary conversation.",
            json!(sessions.limits.temporary_max_turns),
        ),
        field(
            "sessions.limits.temporary_max_bytes",
            "Conversations",
            "Temporary size limit (bytes)",
            "Memory limit for a temporary conversation.",
            json!(sessions.limits.temporary_max_bytes),
        ),
        field(
            "context.enabled",
            "Context",
            "Context management",
            "Compact long conversations automatically.",
            json!(context.enabled),
        ),
        field(
            "context.window_tokens",
            "Context",
            "Context window (tokens)",
            "Maximum total context budget before model-specific limits.",
            json!(context.window_tokens),
        ),
        field(
            "context.reserve_tokens",
            "Context",
            "Reserved tokens",
            "Reserve for instructions, tools and generation.",
            json!(context.reserve_tokens),
        ),
        field(
            "context.response_tokens",
            "Context",
            "Response tokens",
            "Generation budget inside the reserve.",
            json!(context.response_tokens),
        ),
        field(
            "context.new_messages_percent",
            "Context",
            "Live messages (%)",
            "Share of dialogue space reserved for recent messages.",
            json!(context.new_messages_percent),
        ),
        field(
            "context.compact_output_tokens",
            "Context",
            "Compact output tokens",
            "Maximum size of a generated conversation summary.",
            json!(context.compact_output_tokens),
        ),
        field(
            "context.compact_prompt",
            "Context",
            "Compaction instructions",
            "Optional instructions for conversation summaries.",
            json!(context.compact_prompt),
        ),
        field(
            "subagents.enabled",
            "Delegation",
            "Subagents",
            "Allow delegated tasks within their granted capabilities.",
            json!(children.enabled),
        ),
        field(
            "subagents.max_concurrent",
            "Delegation",
            "Concurrent subagents",
            "Maximum simultaneous delegated runs.",
            json!(children.max_concurrent),
        ),
        field(
            "subagents.max_runs_per_turn",
            "Delegation",
            "Runs per turn",
            "Maximum children a main-agent turn can start.",
            json!(children.max_runs_per_turn),
        ),
        field(
            "subagents.default_tool_turns",
            "Delegation",
            "Default tool rounds",
            "Default budget for a delegated run.",
            json!(children.default_tool_turns),
        ),
        field(
            "subagents.max_tool_turns",
            "Delegation",
            "Maximum tool rounds",
            "Upper limit for an explicitly requested delegated budget.",
            json!(children.max_tool_turns),
        ),
        field(
            "subagents.timeout_secs",
            "Delegation",
            "Run timeout (seconds)",
            "Maximum duration of a delegated run.",
            json!(children.timeout_secs),
        ),
        field(
            "skills.cache",
            "Skills & reflection",
            "Cached skills",
            "Number of applied skill bodies kept in memory.",
            json!(d_skills_cache()),
        ),
        field(
            "skills.page",
            "Skills & reflection",
            "Skill page size",
            "Page size when Albert lists skills; zero is treated as one.",
            json!(d_skills_page()),
        ),
        field(
            "reflection.period_secs",
            "Skills & reflection",
            "Reflection interval (seconds)",
            "Interval used for newly scheduled memory reflection.",
            json!(d_reflect()),
        ),
    ]
}
