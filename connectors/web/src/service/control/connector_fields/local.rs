use super::{bounds, choices, field, required};
use crate::api::models::ConfigField;
use serde_json::{json, Value};

pub(super) fn fields(kind: &str) -> Vec<ConfigField> {
    match kind {
        "storage" => vec![
            choices(
                field(
                    "backend",
                    "Storage",
                    "Backend",
                    "Storage implementation.",
                    json!("local"),
                ),
                &["local"],
            ),
            required(field(
                "root",
                "Storage",
                "Storage directory",
                "Directory for stored files, relative to this connector's configuration.",
                Value::Null,
            )),
            field(
                "workspace",
                "Storage",
                "Workspace",
                "Optional workspace for file checkout and promotion.",
                Value::Null,
            ),
        ],
        "forkd" => vec![
            field(
                "workspace",
                "Execution",
                "Working directory",
                "Leave unset to inherit Albert's workspace.",
                Value::Null,
            ),
            field(
                "timeout_secs",
                "Execution",
                "Default timeout (seconds)",
                "Default time limit for scripts.",
                json!(30),
            ),
            field(
                "max_timeout_secs",
                "Execution",
                "Maximum timeout (seconds)",
                "Longest permitted script execution.",
                json!(300),
            ),
            field(
                "max_output_bytes",
                "Limits",
                "Output limit (bytes)",
                "Maximum output per stream.",
                json!(65536),
            ),
            field(
                "fsize_mb",
                "Limits",
                "File size limit (MiB)",
                "Maximum file size a script may write.",
                json!(64),
            ),
            bounds(
                field(
                    "mem_mb",
                    "Limits",
                    "Memory limit (MiB)",
                    "Zero leaves the address-space limit unset.",
                    json!(0),
                ),
                0,
                9_007_199_254_740_991,
            ),
            field(
                "run_as",
                "Process identity",
                "Run as user",
                "Optional operating-system account for scripts.",
                Value::Null,
            ),
            field(
                "run_group",
                "Process identity",
                "Run as group",
                "Optional shared workspace group.",
                Value::Null,
            ),
            field(
                "env_passthrough",
                "Environment",
                "Forwarded variables",
                "Environment variable names to forward, one per line.",
                json!([]),
            ),
        ],
        "sql" => vec![
            required(field(
                "path",
                "Database",
                "Database file",
                "SQLite file for user SQL stores; separate from Albert's internal database.",
                Value::Null,
            )),
            bounds(
                field(
                    "max_stores",
                    "Limits",
                    "Maximum stores",
                    "Maximum number of independent stores.",
                    json!(20),
                ),
                1,
                20,
            ),
            bounds(
                field(
                    "max_rows",
                    "Limits",
                    "Result row limit",
                    "Maximum rows returned by a query.",
                    json!(200),
                ),
                1,
                10000,
            ),
            bounds(
                field(
                    "max_bytes",
                    "Limits",
                    "Result size (bytes)",
                    "Maximum total result size.",
                    json!(262144),
                ),
                1,
                16777216,
            ),
            field(
                "max_cell_bytes",
                "Limits",
                "Cell size (bytes)",
                "Maximum size of a single result cell.",
                json!(65536),
            ),
            field(
                "max_affected_rows",
                "Limits",
                "Change limit (rows)",
                "Maximum actual effects of a write.",
                json!(1000),
            ),
            bounds(
                field(
                    "timeout_ms",
                    "Limits",
                    "Query timeout (ms)",
                    "Execution time limit per query.",
                    json!(5000),
                ),
                1,
                60000,
            ),
            bounds(
                field(
                    "max_pending",
                    "Approvals",
                    "Pending approvals",
                    "Maximum pending destructive operations.",
                    json!(32),
                ),
                1,
                1000,
            ),
            bounds(
                field(
                    "approval_ttl_seconds",
                    "Approvals",
                    "Approval lifetime (seconds)",
                    "How long an exact operation may await confirmation.",
                    json!(300),
                ),
                1,
                3600,
            ),
        ],
        _ => Vec::new(),
    }
}
