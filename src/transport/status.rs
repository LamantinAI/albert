use serde_json::Value;

/// Best-effort HTTP status out of a provider error string. rig drops the HTTP status on
/// a non-2xx and surfaces the raw body, so recover the code from that body:
/// OpenRouter-style `{"error":{"code":400}}` first (parsed from the first brace, past
/// rig's `ProviderError:` prefix), then explicit `HTTP <n>` / `status <n>` markers. Only
/// a 100..=599 is accepted, and a bare integer is never guessed — a wrong code is worse
/// than none, so an unrecognised shape yields `None` (generic message).
pub fn provider_status_code(raw: &str) -> Option<u16> {
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
