use super::LimitWindow;
use serde_json::{from_slice, Value};

pub(super) fn parse_windows(bytes: &[u8]) -> Option<Vec<LimitWindow>> {
    let value: Value = from_slice(bytes).ok()?;
    if !value.is_object() {
        return None;
    }
    let mut windows = Vec::new();
    let mut read = |bucket: &str, limits: &Value| {
        for name in ["primary_window", "secondary_window"] {
            let Some(window) = limits.get(name) else {
                continue;
            };
            let Some(used) = window
                .get("used_percent")
                .and_then(Value::as_f64)
                .filter(|n| n.is_finite() && (0.0..=100.0).contains(n))
            else {
                continue;
            };
            let Some(duration) = window
                .get("limit_window_seconds")
                .and_then(Value::as_u64)
                .filter(|n| *n > 0)
            else {
                continue;
            };
            let Some(reset) = window
                .get("reset_at")
                .and_then(Value::as_i64)
                .filter(|n| *n >= 0)
            else {
                continue;
            };
            let bucket = bucket
                .chars()
                .filter(|c| !c.is_control())
                .take(80)
                .collect();
            windows.push(LimitWindow {
                bucket,
                duration_seconds: duration,
                remaining_percent: 100.0 - used,
                resets_at: reset,
            });
        }
    };
    if let Some(limits) = value.get("rate_limit") {
        read("Subscription", limits);
    }
    if let Some(additional) = value
        .get("additional_rate_limits")
        .and_then(Value::as_array)
    {
        for limits in additional.iter().take(32) {
            let label = limits
                .get("limit_name")
                .and_then(Value::as_str)
                .unwrap_or("Additional limit");
            if let Some(limits) = limits.get("rate_limit") {
                read(label, limits);
            }
        }
    }
    Some(windows)
}
