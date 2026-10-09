use rig::completion::{CompletionError, PromptError};
use serde_json::Value;
use tracing::warn;

use crate::models::{Failure, FailureKind};

/// A short, polite English message for an LLM failure — never the raw provider payload.
/// On a non-2xx the provider (via rig) hands back the whole response body as the error
/// string; dumping that at the user is a wall of JSON that may echo request detail. The
/// full error is already in the log above; the user gets one clean line, with the status
/// code when we can recover it.
pub(super) fn user_facing_llm_error(e: &PromptError) -> String {
    // Our own ceiling, not a provider failure — say so in its own words.
    if let PromptError::MaxTurnsError { .. } = e {
        return "I couldn't finish this within my step budget — try narrowing the request."
            .to_string();
    }
    match provider_status_code(&e.to_string()) {
        Some(code) => format!("LLM provider error: {code}. Please try again in a moment."),
        None => "LLM provider error. Please try again in a moment.".to_string(),
    }
}

/// Best-effort HTTP status out of a provider error string. rig drops the HTTP status on
/// a non-2xx and surfaces the raw body, so recover the code from that body:
/// OpenRouter-style `{"error":{"code":400}}` first (parsed from the first brace, past
/// rig's `ProviderError:` prefix), then explicit `HTTP <n>` / `status <n>` markers. Only
/// a 100..=599 is accepted, and a bare integer is never guessed — a wrong code is worse
/// than none, so an unrecognised shape yields `None` (generic message).
pub(super) fn provider_status_code(raw: &str) -> Option<u16> {
    let in_range = |n: u64| (100..=599).contains(&n).then_some(n as u16);
    // Structured error body.
    if let Some(start) = raw.find('{') {
        if let Ok(v) = serde_json::from_str::<Value>(&raw[start..]) {
            let code = v
                .get("error")
                .and_then(|e| e.get("code"))
                .or_else(|| v.get("code"));
            if let Some(c) = code {
                if let Some(n) = c
                    .as_u64()
                    .or_else(|| c.as_str().and_then(|s| s.parse().ok()))
                {
                    if let Some(code) = in_range(n) {
                        return Some(code);
                    }
                }
            }
        }
    }
    // Explicit textual markers.
    for marker in ["HTTP ", "status: ", "status ", "code: ", "code "] {
        for seg in raw.split(marker).skip(1) {
            let digits: String = seg
                .trim_start()
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            if let Some(code) = digits.parse::<u64>().ok().and_then(in_range) {
                return Some(code);
            }
        }
    }
    None
}

/// True when the completion failed because the server no longer accepts the access
/// token — an HTTP 401 or an explicit `token_expired` from the provider. Detected
/// from the LIVE response only, never from JWT claims: the server can revoke a
/// token well before its `exp` (which is exactly why [`force_refresh`] exists).
pub(super) fn token_rejected(e: &PromptError) -> bool {
    let PromptError::CompletionError(ce) = e else {
        return false;
    };
    let msg = ce.to_string();
    msg.contains("token_expired") || msg.contains("401 Unauthorized")
}

/// A provider-side failure that clears on its own — overload, a rate limit, a dropped
/// connection. Distinct from a malformed request, which no retry can fix.
pub(super) fn transient(e: &PromptError) -> bool {
    let PromptError::CompletionError(ce) = e else {
        return false;
    };
    let msg = ce.to_string();
    let low = msg.to_ascii_lowercase();
    if low.contains("server_is_overloaded")
        || low.contains("overloaded")
        || low.contains("rate_limit")
        || low.contains("temporarily unavailable")
        || low.contains("timed out")
        || low.contains("connection reset")
        || low.contains("connection closed")
    {
        return true;
    }
    // Numeric codes go through the parser instead of a substring search: "500" turns
    // up in harmless error prose, and a retry would fire for nothing.
    matches!(
        provider_status_code(&msg),
        Some(429 | 500 | 502 | 503 | 504)
    )
}

/// Classify before selecting another model; malformed generic requests and tool
/// errors must not become repeated attempts against every provider.
pub(super) fn model_failure(error: PromptError) -> Failure {
    let raw = error.to_string();
    let lower = raw.to_ascii_lowercase();
    let completion = matches!(error, PromptError::CompletionError(_));
    let capability = [
        "does not support",
        "not supported",
        "unsupported",
        "no endpoints found",
        "context_length_exceeded",
        "maximum context length",
    ]
    .iter()
    .any(|hint| lower.contains(hint));
    let kind = if transient(&error)
        || matches!(
            &error,
            PromptError::CompletionError(CompletionError::HttpError(_))
        ) {
        FailureKind::Transient
    } else if completion
        && (token_rejected(&error) || matches!(provider_status_code(&raw), Some(401 | 403)))
    {
        FailureKind::Authentication
    } else if (completion && capability)
        || matches!(
            &error,
            PromptError::CompletionError(
                CompletionError::ResponseError(_) | CompletionError::JsonError(_)
            )
        )
    {
        FailureKind::Incompatible
    } else if completion && matches!(provider_status_code(&raw), Some(404)) {
        FailureKind::Unavailable
    } else {
        FailureKind::Fatal
    };
    // The chat gets a short, safe summary; the operator needs the provider's own
    // words to tell an overload from a malformed request, so they go to the log.
    warn!(kind = ?kind, error = %raw, "model attempt failed");
    Failure {
        kind,
        message: format!("{kind:?}: {}", user_facing_llm_error(&error)),
    }
}
