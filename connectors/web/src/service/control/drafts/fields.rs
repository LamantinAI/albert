use albert_core::{
    config::d_max_turns, config::d_reflect, config::d_skills_cache, config::d_skills_page,
    context::Settings as ContextSettings, sessions::settings::SessionSettings, subagents::Limits,
};
use chrono_tz::UTC;
use serde_json::{json, Value};

use crate::{api::models::ConfigField, api::models::ConfigFieldKind};

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
        required: None,
        choices: None,
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
            "Timezone for dates and reminders.",
            json!(UTC.name()),
        ),
        field(
            "stream_status",
            "General",
            "Live progress",
            "Show tool activity during a task.",
            json!(true),
        ),
        field(
            "agent.max_tool_turns",
            "General",
            "Tool rounds per turn",
            "Tool budget for one request.",
            json!(d_max_turns()),
        ),
        field(
            "sessions.titles.enabled",
            "Conversations",
            "Automatic titles",
            "Name new chats automatically.",
            json!(sessions.titles.enabled),
        ),
        field(
            "sessions.limits.temporary_max_sessions",
            "Conversations",
            "Temporary conversations",
            "Temporary chats kept at the same time.",
            json!(sessions.limits.temporary_max_sessions),
        ),
        field(
            "sessions.limits.temporary_max_turns",
            "Conversations",
            "Temporary turn limit",
            "Conversation length before the temporary limit.",
            json!(sessions.limits.temporary_max_turns),
        ),
        field(
            "sessions.limits.temporary_max_bytes",
            "Conversations",
            "Temporary size limit (bytes)",
            "Storage budget for one temporary chat.",
            json!(sessions.limits.temporary_max_bytes),
        ),
        field(
            "context.enabled",
            "Context",
            "Context management",
            "Summarize long conversations automatically.",
            json!(context.enabled),
        ),
        field(
            "context.window_tokens",
            "Context",
            "Context window (tokens)",
            "Context budget, also limited by the selected model.",
            json!(context.window_tokens),
        ),
        ConfigField {
            default_value: Value::Null,
            ..field(
                "context.reserve_tokens",
                "Context",
                "Reserved tokens",
                "Space for instructions, tools and the response. Leave empty to reserve up to 65,536 tokens or half the model window, whichever is smaller.",
                json!(context.effective_reserve()),
            )
        },
        field(
            "context.response_tokens",
            "Context",
            "Response tokens",
            "Response budget within the reserved space.",
            json!(context.response_tokens),
        ),
        field(
            "context.new_messages_percent",
            "Context",
            "Live messages (%)",
            "Dialogue space kept for recent messages.",
            json!(context.new_messages_percent),
        ),
        field(
            "context.compact_output_tokens",
            "Context",
            "Compact output tokens",
            "Maximum conversation summary size.",
            json!(context.compact_output_tokens),
        ),
        field(
            "context.compact_prompt",
            "Context",
            "Compaction instructions",
            "Optional guidance for summaries.",
            json!(context.compact_prompt),
        ),
        field(
            "subagents.enabled",
            "Delegation",
            "Subagents",
            "Allow tasks to be delegated to subagents.",
            json!(children.enabled),
        ),
        field(
            "subagents.max_concurrent",
            "Delegation",
            "Concurrent subagents",
            "Tasks that can run at the same time.",
            json!(children.max_concurrent),
        ),
        field(
            "subagents.max_runs_per_turn",
            "Delegation",
            "Runs per turn",
            "Subagent runs allowed for one request.",
            json!(children.max_runs_per_turn),
        ),
        field(
            "subagents.default_tool_turns",
            "Delegation",
            "Default tool rounds",
            "Tool budget for a new subagent run.",
            json!(children.default_tool_turns),
        ),
        field(
            "subagents.max_tool_turns",
            "Delegation",
            "Maximum tool rounds",
            "Largest tool budget a subagent can request.",
            json!(children.max_tool_turns),
        ),
        field(
            "subagents.timeout_secs",
            "Delegation",
            "Run timeout (seconds)",
            "Time allowed for one subagent run.",
            json!(children.timeout_secs),
        ),
        field(
            "skills.cache",
            "Skills & reflection",
            "Cached skills",
            "Applied skills kept ready for reuse.",
            json!(d_skills_cache()),
        ),
        field(
            "skills.page",
            "Skills & reflection",
            "Skill page size",
            "Skills listed per page; zero uses one.",
            json!(d_skills_page()),
        ),
        field(
            "reflection.period_secs",
            "Skills & reflection",
            "Reflection interval (seconds)",
            "Schedule for new memory reflection runs.",
            json!(d_reflect()),
        ),
    ]
}
