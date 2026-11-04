use super::{bounds, choices, field};
use crate::api::models::ConfigField;
use serde_json::{json, Value};

pub(super) fn fields(kind: &str) -> Vec<ConfigField> {
    match kind {
        "speak" => vec![choices(
            field(
                "voice",
                "Speech",
                "Default voice",
                "Voice used when a request does not specify one.",
                json!("cove"),
            ),
            &[
                "cove", "juniper", "maple", "spruce", "ember", "vale", "breeze", "arbor", "sol",
            ],
        )],
        "transcribe" => vec![
            field(
                "language",
                "Transcription",
                "Language hint",
                "Optional language code; leave unset for automatic detection.",
                Value::Null,
            ),
            bounds(
                field(
                    "chunk_secs",
                    "Long recordings",
                    "Chunk duration (seconds)",
                    "Target duration before splitting on pauses.",
                    json!(300.0),
                ),
                60,
                1380,
            ),
            bounds(
                field(
                    "parallel_uploads",
                    "Long recordings",
                    "Parallel uploads",
                    "Number of chunks uploaded at once.",
                    json!(4),
                ),
                1,
                8,
            ),
        ],
        "imagegen" => vec![
            field(
                "size",
                "Images",
                "Default size",
                "Optional supported image size, such as 1024x1024 or 1536x1024.",
                Value::Null,
            ),
            choices(
                field(
                    "quality",
                    "Images",
                    "Quality",
                    "Leave unset to use the provider's default.",
                    Value::Null,
                ),
                &["auto", "low", "medium", "high"],
            ),
            choices(
                field(
                    "background",
                    "Images",
                    "Background",
                    "Leave unset to use the provider's default.",
                    Value::Null,
                ),
                &["auto", "transparent", "opaque"],
            ),
        ],
        _ => Vec::new(),
    }
}
