use super::{bounds, choices, field, required};
use crate::api::models::ConfigField;
use serde_json::{json, Value};

pub(super) fn fields(kind: &str) -> Vec<ConfigField> {
    match kind {
        "search" => vec![
            field(
                "default_engine",
                "Search",
                "Default engine",
                "Optional name of a declared engine; otherwise the first available engine is used.",
                Value::Null,
            ),
            field(
                "timeout_secs",
                "Search",
                "Timeout (seconds)",
                "Time limit for a search request.",
                json!(15),
            ),
            field(
                "default_limit",
                "Results",
                "Default result count",
                "Number of results when a request does not specify a limit.",
                json!(10),
            ),
            field(
                "max_limit",
                "Results",
                "Maximum result count",
                "Upper limit on requested results.",
                json!(25),
            ),
        ],
        "browser" => vec![
            field(
                "headless",
                "Browser",
                "Headless mode",
                "Run without a visible browser window.",
                json!(true),
            ),
            field(
                "sandbox",
                "Browser",
                "Browser sandbox",
                "Use Chromium's process sandbox when supported by the host.",
                json!(false),
            ),
            field(
                "data_dir",
                "Browser",
                "Browser data directory",
                "Download and profile directory relative to this connector's configuration.",
                json!("browser-data"),
            ),
            field(
                "executable",
                "Browser",
                "Browser executable",
                "Optional existing Chromium executable instead of the managed installation.",
                Value::Null,
            ),
            field(
                "timeout_secs",
                "Requests",
                "Default timeout (seconds)",
                "Default time limit for fetching a page.",
                json!(30),
            ),
            field(
                "max_timeout_secs",
                "Requests",
                "Maximum timeout (seconds)",
                "Longest timeout a request may select.",
                json!(90),
            ),
        ],
        "http" => vec![
            required(field(
                "base_url",
                "Connection",
                "Service URL",
                "Base URL for this HTTP service.",
                Value::Null,
            )),
            bounds(
                field(
                    "timeout.request_ms",
                    "Requests",
                    "Request timeout (ms)",
                    "Optional timeout for HTTP requests.",
                    Value::Null,
                ),
                1,
                3600000,
            ),
            field(
                "models_dir",
                "Advanced",
                "Model definitions directory",
                "Optional directory containing request and response models.",
                Value::Null,
            ),
        ],
        "caldav" => vec![
            field(
                "collection",
                "Calendar",
                "Calendar collection URL",
                "Use a direct calendar URL or configure discovery below.",
                Value::Null,
            ),
            field(
                "base_url",
                "Calendar",
                "Discovery URL",
                "Server URL used to discover calendars when no collection is specified.",
                Value::Null,
            ),
            field(
                "calendar",
                "Calendar",
                "Calendar name",
                "Optional display name used to select a discovered calendar.",
                Value::Null,
            ),
            field(
                "timezone",
                "Calendar",
                "Display timezone",
                "IANA timezone for displayed event times.",
                json!("UTC"),
            ),
            bounds(
                field(
                    "reminder_minutes",
                    "Calendar",
                    "Default reminder (minutes)",
                    "Optional lead time for new events; -1 disables the reminder.",
                    Value::Null,
                ),
                -1,
                525600,
            ),
            choices(
                field(
                    "auth",
                    "Authentication",
                    "Authentication method",
                    "Use an app password or existing OAuth credentials.",
                    json!("none"),
                ),
                &["none", "basic", "bearer", "oauth2"],
            ),
            field(
                "login",
                "Authentication",
                "Username",
                "Account name for basic authentication.",
                Value::Null,
            ),
            field(
                "password_env",
                "Credential references",
                "Password variable",
                "Environment reference holding the app password.",
                Value::Null,
            ),
            field(
                "token_env",
                "Credential references",
                "Bearer token variable",
                "Environment reference holding a bearer token.",
                Value::Null,
            ),
            field(
                "token_url",
                "OAuth configuration",
                "Token URL",
                "OAuth token endpoint.",
                Value::Null,
            ),
            field(
                "client_id",
                "OAuth configuration",
                "Client ID",
                "OAuth application identifier.",
                Value::Null,
            ),
            field(
                "client_secret_env",
                "Credential references",
                "Client secret variable",
                "Environment reference holding the client secret.",
                Value::Null,
            ),
            field(
                "refresh_token_env",
                "Credential references",
                "Refresh token variable",
                "Environment reference holding the refresh token.",
                Value::Null,
            ),
        ],
        "mail" => mail(),
        _ => Vec::new(),
    }
}

fn mail() -> Vec<ConfigField> {
    let mut fields = vec![
        required(field(
            "imap_host",
            "Incoming mail",
            "IMAP server",
            "Hostname of the IMAP server.",
            Value::Null,
        )),
        bounds(
            field(
                "imap_port",
                "Incoming mail",
                "IMAP port",
                "Port for implicit TLS.",
                json!(993),
            ),
            1,
            65535,
        ),
        field(
            "mailbox",
            "Incoming mail",
            "Mailbox",
            "Mailbox opened by default.",
            json!("INBOX"),
        ),
        field(
            "smtp_host",
            "Outgoing mail",
            "SMTP server",
            "Leave unset to use the IMAP server.",
            Value::Null,
        ),
        bounds(
            field(
                "smtp_port",
                "Outgoing mail",
                "SMTP port",
                "Port for implicit TLS.",
                json!(465),
            ),
            1,
            65535,
        ),
        field(
            "from",
            "Outgoing mail",
            "Sender address",
            "Optional sender address; otherwise the IMAP account is used.",
            Value::Null,
        ),
    ];
    for (key, label, required_key) in [
        ("imap_user_env", "IMAP username variable", true),
        ("imap_pass_env", "IMAP password variable", true),
        ("smtp_user_env", "SMTP username variable", false),
        ("smtp_pass_env", "SMTP password variable", false),
        ("from_env", "Sender address variable", false),
    ] {
        let value = field(
            key,
            "Credential references",
            label,
            "Named environment reference; optional SMTP credentials inherit the IMAP account.",
            Value::Null,
        );
        fields.push(if required_key { required(value) } else { value });
    }
    fields
}
