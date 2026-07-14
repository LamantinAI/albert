//! Google Calendar reminders — an `add_calendar_event` tool that creates an event
//! (with a popup reminder) in the owner's Google Calendar, mirroring the openclaw
//! `reminder-calendar` skill. The event syncs to Apple Calendar when the Google
//! account is attached on the device.
//!
//! Auth is the OAuth2 refresh-token flow, same as openclaw's script: a long-lived
//! refresh token (+ client id/secret) is exchanged for a short-lived access token on
//! each call, then `events.insert` posts the event. Credentials come from
//! [`GcalConfig`] (secrets from the environment); the tool is only attached when
//! they are present.

use std::convert::Infallible;

use chrono::{DateTime, Duration, FixedOffset};
use reqwest::Client as HttpClient;
use rig::{completion::ToolDefinition, tool::Tool};
use serde::Deserialize;
use serde_json::{json, Value};

const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

/// Google Calendar OAuth credentials + event defaults.
#[derive(Clone)]
pub struct GcalConfig {
    pub client_id: String,
    pub client_secret: String,
    pub refresh_token: String,
    /// Calendar to write to (`primary` or a calendar id / address).
    pub calendar_id: String,
    /// IANA zone sent to the API (e.g. `Europe/Moscow`).
    pub timezone: String,
    /// Offset appended when a `start`/`end` has none (e.g. `+03:00`).
    pub tz_offset: String,
    /// Default popup lead time in minutes.
    pub reminder_min: i64,
}

/// The rig tool. Cheap to clone (just the config).
#[derive(Clone)]
pub struct GcalTool {
    config: GcalConfig,
}

impl GcalTool {
    pub fn new(config: GcalConfig) -> Self {
        Self { config }
    }
}

#[derive(Debug, Deserialize)]
pub struct GcalArgs {
    pub summary: String,
    pub start: String,
    #[serde(default)]
    pub end: Option<String>,
    #[serde(default)]
    pub duration_min: Option<i64>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub reminder_min: Option<i64>,
}

impl Tool for GcalTool {
    const NAME: &'static str = "add_calendar_event";
    type Error = Infallible;
    type Args = GcalArgs;
    type Output = Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: format!(
                "Create an event with a popup reminder in the owner's Google Calendar \
                 (it syncs to their phone / Apple Calendar). Use whenever the user asks to \
                 remind them, set a reminder, not-forget, add to calendar, or record a \
                 meeting, AND a concrete date and time can be determined. `start` is ISO-8601; \
                 include the offset (default {off}), e.g. \"2026-06-09T15:00:00{off}\". For \
                 relative times (\"in an hour\", \"tomorrow at 15:00\") compute the absolute \
                 time first. If no time is given and cannot be inferred, ask for it before \
                 calling. Returns a link to the created event.",
                off = self.config.tz_offset,
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "summary": { "type": "string", "description": "Event title / what to be reminded of" },
                    "start": { "type": "string", "description": "ISO-8601 start, e.g. 2026-06-09T15:00:00+03:00" },
                    "end": { "type": "string", "description": "Optional ISO-8601 end" },
                    "duration_min": { "type": "integer", "description": "Duration in minutes if no end (default 30)" },
                    "description": { "type": "string" },
                    "location": { "type": "string" },
                    "reminder_min": { "type": "integer", "description": "Popup this many minutes before (default 10; -1 = calendar default)" }
                },
                "required": ["summary", "start"]
            }),
        }
    }

    async fn call(&self, args: GcalArgs) -> Result<Value, Infallible> {
        // Failures come back as data (never Err), so the agent tool-loop keeps going
        // and the model can report or retry — same convention as the scratchpad tools.
        Ok(self.create(args).await.unwrap_or_else(|e| json!({ "ok": false, "error": e })))
    }
}

impl GcalTool {
    async fn create(&self, args: GcalArgs) -> Result<Value, String> {
        let start = self.ensure_offset(&args.start);
        let end = match &args.end {
            Some(e) => self.ensure_offset(e),
            None => {
                let dur = args.duration_min.unwrap_or(30).max(1);
                let dt = DateTime::<FixedOffset>::parse_from_rfc3339(&start)
                    .map_err(|e| format!("could not parse start {start:?}: {e}"))?;
                (dt + Duration::minutes(dur)).to_rfc3339()
            }
        };

        let reminder = args.reminder_min.unwrap_or(self.config.reminder_min);
        let reminders = if reminder >= 0 {
            json!({ "useDefault": false, "overrides": [{ "method": "popup", "minutes": reminder }] })
        } else {
            json!({ "useDefault": true })
        };
        let mut event = json!({
            "summary": args.summary,
            "start": { "dateTime": start, "timeZone": self.config.timezone },
            "end":   { "dateTime": end,   "timeZone": self.config.timezone },
            "reminders": reminders,
        });
        if let Some(d) = args.description {
            event["description"] = json!(d);
        }
        if let Some(l) = args.location {
            event["location"] = json!(l);
        }

        let http = HttpClient::new();
        let access = self.access_token(&http).await?;
        let url = format!(
            "https://www.googleapis.com/calendar/v3/calendars/{}/events",
            percent_encode(&self.config.calendar_id)
        );
        let resp = http
            .post(&url)
            .bearer_auth(&access)
            .json(&event)
            .send()
            .await
            .map_err(|e| format!("events.insert request failed: {e}"))?;
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            let detail = body.get("error").unwrap_or(&body);
            return Err(format!("events.insert failed (HTTP {status}): {detail}"));
        }
        Ok(json!({
            "ok": true,
            "id": body.get("id"),
            "link": body.get("htmlLink"),
            "start": start,
        }))
    }

    /// Exchange the refresh token for a short-lived access token.
    async fn access_token(&self, http: &HttpClient) -> Result<String, String> {
        let resp = http
            .post(TOKEN_URL)
            .form(&[
                ("client_id", self.config.client_id.as_str()),
                ("client_secret", self.config.client_secret.as_str()),
                ("refresh_token", self.config.refresh_token.as_str()),
                ("grant_type", "refresh_token"),
            ])
            .send()
            .await
            .map_err(|e| format!("token refresh request failed: {e}"))?;
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(Value::Null);
        body.get("access_token")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| {
                format!(
                    "token refresh failed (HTTP {status}); the Google refresh token may be \
                     revoked/expired — re-run the OAuth consent"
                )
            })
    }

    /// Append the configured offset when `s` carries none, so a bare local time
    /// (`2026-06-09T15:00:00`) is treated as the owner's timezone.
    fn ensure_offset(&self, s: &str) -> String {
        let t = s.trim();
        if DateTime::parse_from_rfc3339(t).is_ok() {
            t.to_string()
        } else {
            format!("{t}{}", self.config.tz_offset)
        }
    }
}

/// Percent-encode a path segment (calendar ids are ASCII: `primary` or an address).
fn percent_encode(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            _ => format!("%{:02X}", c as u32),
        })
        .collect()
}
