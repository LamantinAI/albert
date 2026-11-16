//! Safe compact presentation of recorded tool actions.
use serde_json::{from_str, Value};
/// Tool args as shown in the live status and recorded in the action log — with secret
/// values redacted so `config_set_secret`'s value never reaches chat or history.
pub(super) fn display_args(tool_name: &str, args: &str) -> String {
    if tool_name == "config_set_secret" {
        let name = from_str::<Value>(args)
            .ok()
            .and_then(|v| v.get("name").and_then(|n| n.as_str()).map(str::to_string))
            .unwrap_or_else(|| "?".to_string());
        return format!("{{name: {name}, value: <redacted>}}");
    }
    args.to_string()
}
/// Compress a tool result to a few tokens — `ok` / `err: …` when the payload says so,
/// else a hard clip. Results are the expensive half of an action record; args are kept.
pub(super) fn summarize_result(result: &str) -> String {
    if let Ok(v) = from_str::<Value>(result) {
        if let Some(e) = v.get("error").and_then(|e| e.as_str()) {
            return format!("err: {}", clip(e, 80));
        }
        match v.get("ok").and_then(Value::as_bool) {
            Some(true) => return "ok".to_string(),
            Some(false) => return "err".to_string(),
            None => {}
        }
    }
    clip(&result.replace('\n', " "), 80)
}
/// Clip to at most `max` chars on a char boundary, marking the cut with an ellipsis.
pub(super) fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    format!("{cut}…")
}
