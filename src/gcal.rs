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

use chrono::{DateTime, Duration, FixedOffset, NaiveDateTime, TimeZone};
use reqwest::Client as HttpClient;
use rig::{completion::ToolDefinition, tool::Tool};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::warn;

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
        // Also logged, so a production failure is visible in the container logs.
        Ok(match self.create(args).await {
            Ok(v) => v,
            Err(e) => {
                warn!(error = %e, "add_calendar_event failed");
                json!({ "ok": false, "error": e })
            }
        })
    }
}

impl GcalTool {
    async fn create(&self, args: GcalArgs) -> Result<Value, String> {
        // Normalize to a canonical RFC3339 (always seconds + a single offset). The
        // model sends times in many shapes — with/without seconds, with/without an
        // offset — and Google rejects anything that isn't a full RFC3339.
        let start_dt = self.parse_flexible(&args.start).ok_or_else(|| {
            format!("could not parse start time {:?} — expected e.g. 2026-07-15T14:00:00+03:00", args.start)
        })?;
        let end_dt = match &args.end {
            Some(e) => self
                .parse_flexible(e)
                .ok_or_else(|| format!("could not parse end time {e:?}"))?,
            None => start_dt + Duration::minutes(args.duration_min.unwrap_or(30).max(1)),
        };
        let start = start_dt.to_rfc3339();
        let end = end_dt.to_rfc3339();

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

    /// Parse the model's time in whatever shape it sent — a full RFC3339, or a naive
    /// datetime (with or without seconds, `T`- or space-separated), with or without a
    /// trailing offset — into a canonical `DateTime<FixedOffset>`. A missing offset
    /// defaults to the configured zone. Callers then emit `to_rfc3339()`, which always
    /// has seconds + one offset, so Google never sees a malformed time.
    fn parse_flexible(&self, s: &str) -> Option<DateTime<FixedOffset>> {
        let t = s.trim();
        if let Ok(dt) = DateTime::parse_from_rfc3339(t) {
            return Some(dt);
        }
        let (body, off) = split_offset(t);
        let offset = offset_of(off.unwrap_or(&self.config.tz_offset))?;
        const FORMATS: [&str; 4] = [
            "%Y-%m-%dT%H:%M:%S",
            "%Y-%m-%dT%H:%M",
            "%Y-%m-%d %H:%M:%S",
            "%Y-%m-%d %H:%M",
        ];
        FORMATS.iter().find_map(|fmt| {
            NaiveDateTime::parse_from_str(body, fmt)
                .ok()
                .and_then(|naive| offset.from_local_datetime(&naive).single())
        })
    }
}

/// Split a trailing timezone offset (`Z`, `+HH:MM`, `-HH:MM`) off a datetime string as
/// `(body, offset)`. Only scans after the `T`/space, so the date's own dashes aren't
/// mistaken for an offset.
fn split_offset(t: &str) -> (&str, Option<&str>) {
    if let Some(body) = t.strip_suffix('Z') {
        return (body, Some("Z"));
    }
    let time_start = t.find(['T', ' ']).map(|i| i + 1).unwrap_or(0);
    if let Some(rel) = t[time_start..].rfind(['+', '-']) {
        let idx = time_start + rel;
        return (&t[..idx], Some(&t[idx..]));
    }
    (t, None)
}

/// Resolve an offset string (`+03:00`, `-05:00`, `Z`) to a `FixedOffset`, reusing
/// chrono's own RFC3339 offset parsing.
fn offset_of(s: &str) -> Option<FixedOffset> {
    let s = if s == "Z" { "+00:00" } else { s };
    DateTime::parse_from_rfc3339(&format!("2020-01-01T00:00:00{s}"))
        .ok()
        .map(|dt| *dt.offset())
}

#[cfg(test)]
mod tests {
    use super::{offset_of, split_offset, GcalConfig, GcalTool};

    fn tool() -> GcalTool {
        GcalTool::new(GcalConfig {
            client_id: String::new(),
            client_secret: String::new(),
            refresh_token: String::new(),
            calendar_id: "primary".into(),
            timezone: "Europe/Moscow".into(),
            tz_offset: "+03:00".into(),
            reminder_min: 10,
        })
    }

    #[test]
    fn normalizes_every_time_shape_to_rfc3339_with_seconds() {
        let t = tool();
        // (input, expected canonical rfc3339)
        let cases = [
            ("2026-07-15T14:00:00+03:00", "2026-07-15T14:00:00+03:00"), // full
            ("2026-07-15T14:00+03:00", "2026-07-15T14:00:00+03:00"),    // no seconds (the bug)
            ("2026-07-15T14:00:00", "2026-07-15T14:00:00+03:00"),       // no offset
            ("2026-07-15T14:00", "2026-07-15T14:00:00+03:00"),          // neither
            ("2026-07-15 14:00", "2026-07-15T14:00:00+03:00"),          // space-separated
        ];
        for (input, want) in cases {
            let got = t.parse_flexible(input).unwrap().to_rfc3339();
            assert_eq!(got, want, "input {input:?}");
        }
        assert!(t.parse_flexible("not a time").is_none());
    }

    #[test]
    fn split_offset_only_looks_after_the_time() {
        assert_eq!(split_offset("2026-07-15T14:00+03:00"), ("2026-07-15T14:00", Some("+03:00")));
        assert_eq!(split_offset("2026-07-15T14:00-05:00"), ("2026-07-15T14:00", Some("-05:00")));
        assert_eq!(split_offset("2026-07-15T14:00:00Z"), ("2026-07-15T14:00:00", Some("Z")));
        assert_eq!(split_offset("2026-07-15T14:00"), ("2026-07-15T14:00", None));
        assert!(offset_of("+03:00").is_some());
        assert!(offset_of("Z").is_some());
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
