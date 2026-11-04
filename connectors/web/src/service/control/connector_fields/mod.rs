mod local;
mod media;
mod network;

use serde_json::{json, Value};

use crate::api::models::{ConfigField, ConfigFieldKind};

pub(super) const CONNECTOR_TYPES: &[(&str, &str)] = &[
    ("telegram", "Telegram"),
    ("search", "Web search"),
    ("browser", "Browser"),
    ("storage", "File storage"),
    ("forkd", "Script runner"),
    ("sql", "SQL storage"),
    ("caldav", "Calendar"),
    ("mail", "Email"),
    ("http", "HTTP service"),
    ("transcribe", "Speech recognition"),
    ("speak", "Voice"),
    ("imagegen", "Image generation"),
];

fn field(path: &str, group: &str, label: &str, description: &str, default: Value) -> ConfigField {
    let kind = if default.is_boolean() {
        ConfigFieldKind::Boolean
    } else if default.is_i64() {
        ConfigFieldKind::Integer
    } else if default.is_f64() {
        ConfigFieldKind::Number
    } else if default.is_array() {
        ConfigFieldKind::TextList
    } else {
        ConfigFieldKind::Text
    };
    let numeric = matches!(kind, ConfigFieldKind::Integer | ConfigFieldKind::Number);
    ConfigField {
        path: format!("connector.{path}"),
        group: group.into(),
        label: label.into(),
        description: description.into(),
        kind,
        value: None,
        default_value: default,
        minimum: numeric.then_some(1),
        maximum: numeric.then_some(9_007_199_254_740_991),
        required: None,
        choices: None,
    }
}
fn required(mut field: ConfigField) -> ConfigField {
    field.required = Some(true);
    field
}
fn choices(mut field: ConfigField, values: &[&str]) -> ConfigField {
    field.choices = Some(values.iter().map(|s| (*s).into()).collect());
    field
}
fn bounds(mut field: ConfigField, min: i64, max: i64) -> ConfigField {
    if matches!(field.kind, ConfigFieldKind::Text) && field.default_value.is_null() {
        field.kind = ConfigFieldKind::Integer;
    }
    field.minimum = Some(min);
    field.maximum = Some(max);
    field
}

pub(super) fn connector_fields(kind: &str) -> Vec<ConfigField> {
    match kind {
        "telegram" => vec![
            field(
                "address_names",
                "Conversation",
                "Address names",
                "Names that address Albert in groups, one per line.",
                json!([]),
            ),
            bounds(
                field(
                    "batch_debounce_ms",
                    "Message batching",
                    "Message delay (ms)",
                    "Wait briefly for consecutive messages.",
                    json!(500),
                ),
                0,
                60_000,
            ),
            bounds(
                field(
                    "batch_max_wait_ms",
                    "Message batching",
                    "Maximum batching delay (ms)",
                    "Maximum time before starting a turn.",
                    json!(3000),
                ),
                0,
                60_000,
            ),
        ],
        "storage" | "forkd" | "sql" => local::fields(kind),
        "speak" | "transcribe" | "imagegen" => media::fields(kind),
        _ => network::fields(kind),
    }
}
