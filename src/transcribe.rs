//! Hearing: turning a voice message into text with the ChatGPT subscription that
//! already drives the model.
//!
//! No Codex model accepts audio (every one advertises `input_modalities:
//! ["text", "image"]`), so a voice note can't simply be handed to the agent the
//! way a photo can. It has to become text first. The desktop ChatGPT app's own
//! dictation endpoint does exactly that on the *subscription* token — the same
//! `auth.json` Albert already reads for the model — so hearing costs no API key
//! and no per-minute billing.
//!
//! That is also why hearing is gated on [`AuthMode::Subscription`](crate::config::AuthMode):
//! with an API key there is simply no token this endpoint would accept.

use std::time::Duration;

use chrono::Utc;
use reqwest::Client as HttpClient;
use serde_json::Value;
use tracing::warn;

use crate::{
    error::{Error, Result},
    openai_auth::Subscription,
};

const TRANSCRIBE_URL: &str = "https://chatgpt.com/backend-api/transcribe";

/// What the endpoint claims to be, matching the desktop client it belongs to.
const ORIGINATOR: &str = "Codex Desktop";

/// Longest voice message transcribed inline, in seconds.
///
/// The endpoint accepts roughly 23 minutes; past that it answers `500`. Worse, a
/// long-but-accepted upload can come back `200` with the transcript **silently cut
/// off mid-sentence**, so the ceiling here is deliberately below the failure point
/// rather than at it. Longer recordings belong to the transcription skill, which
/// splits them on silence and stitches the pieces back together.
pub const MAX_INLINE_SECS: u32 = 20 * 60;

/// Upload budget: transcription runs ~15-30x real time, so even a 20-minute note
/// lands well inside this. Generous because a slow link, not the ASR, is the risk.
const TIMEOUT: Duration = Duration::from_secs(300);

/// Transcribe `audio` via the subscription dictation endpoint, returning the text.
///
/// `language` is an optional hint (e.g. `"ru"`); omitted, the endpoint detects it.
pub async fn transcribe(
    audio: &[u8],
    filename: &str,
    content_type: &str,
    language: Option<&str>,
    sub: &Subscription,
) -> Result<String> {
    let boundary = boundary();
    let body = multipart_body(&boundary, audio, filename, content_type, language);

    let resp = HttpClient::new()
        .post(TRANSCRIBE_URL)
        .timeout(TIMEOUT)
        .header("Content-Type", format!("multipart/form-data; boundary={boundary}"))
        .header("Authorization", format!("Bearer {}", sub.access_token))
        .header("chatgpt-account-id", sub.account_id.as_str())
        .header("originator", ORIGINATOR)
        .body(body)
        .send()
        .await
        .map_err(|e| Error::Transcribe(format!("request failed: {e}")))?;

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        // The endpoint reports an over-long upload as a plain 500, so say what is
        // most likely rather than echoing an opaque status.
        return Err(Error::Transcribe(format!(
            "HTTP {status} (audio too long or the subscription token was refused): {}",
            snippet(&text)
        )));
    }
    let parsed: Value = serde_json::from_str(&text)
        .map_err(|e| Error::Transcribe(format!("bad response body: {e}: {}", snippet(&text))))?;
    let transcript = parsed
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Transcribe(format!("no `text` in response: {}", snippet(&text))))?
        .trim()
        .to_string();

    if looks_truncated(&transcript) {
        // Not an error: partial text still beats none, but it must not pass silently.
        warn!(chars = transcript.len(), "transcript may be truncated (no terminal punctuation)");
    }
    Ok(transcript)
}

/// A unique multipart boundary, shaped like the desktop client's.
fn boundary() -> String {
    format!("----codex-transcribe-{}", Utc::now().timestamp_nanos_opt().unwrap_or_default())
}

/// Assemble the `multipart/form-data` body by hand — one file part plus an
/// optional language field.
///
/// Note there is deliberately no `x-codex-base64` header anywhere: that one
/// belongs to the app's internal Electron bridge (which base64-encodes the body).
/// A direct HTTP call sends plain multipart.
fn multipart_body(
    boundary: &str,
    audio: &[u8],
    filename: &str,
    content_type: &str,
    language: Option<&str>,
) -> Vec<u8> {
    let mut body = Vec::with_capacity(audio.len() + 512);
    if let Some(lang) = language {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"language\"\r\n\r\n{lang}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\n\
             Content-Type: {content_type}\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(audio);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

/// A transcript that ends mid-sentence is the endpoint's silent-truncation
/// signature. Short replies ("Да.", "Ок") are normal, so only longer text counts.
fn looks_truncated(text: &str) -> bool {
    const TERMINAL: [char; 8] = ['.', '!', '?', '…', '"', ')', ':', ';'];
    text.chars().count() > 200 && !text.trim_end().ends_with(TERMINAL)
}

/// First 200 chars of a response body, for error messages.
fn snippet(s: &str) -> String {
    s.chars().take(200).collect()
}

#[cfg(test)]
mod tests {
    use super::{boundary, looks_truncated, multipart_body};

    #[test]
    fn body_carries_the_file_part_and_the_audio_bytes() {
        let audio = [0xDEu8, 0xAD, 0xBE, 0xEF];
        let body = multipart_body("BND", &audio, "voice.ogg", "audio/ogg", None);
        let rendered = String::from_utf8_lossy(&body);
        assert!(rendered.contains("--BND\r\nContent-Disposition: form-data; name=\"file\""));
        assert!(rendered.contains("filename=\"voice.ogg\""));
        assert!(rendered.contains("Content-Type: audio/ogg"));
        assert!(rendered.ends_with("\r\n--BND--\r\n"), "body must close the boundary");
        // The raw bytes survive verbatim between the headers and the closing boundary.
        assert!(body.windows(audio.len()).any(|w| w == audio));
        assert!(!rendered.contains("name=\"language\""), "no language field when unset");
    }

    #[test]
    fn language_hint_precedes_the_file_part() {
        let body = multipart_body("BND", &[1], "a.ogg", "audio/ogg", Some("ru"));
        let rendered = String::from_utf8_lossy(&body);
        let lang_at = rendered.find("name=\"language\"").expect("language part present");
        let file_at = rendered.find("name=\"file\"").expect("file part present");
        assert!(lang_at < file_at, "the file part must come last");
        assert!(rendered.contains("\r\n\r\nru\r\n"));
    }

    #[test]
    fn truncation_heuristic_ignores_short_answers() {
        assert!(!looks_truncated("Да"), "a short reply is not a truncated one");
        let long_open = "слово ".repeat(60); // >200 chars, ends without punctuation
        assert!(looks_truncated(&long_open));
        assert!(!looks_truncated(&format!("{long_open}.")));
    }

    #[test]
    fn boundary_is_shaped_like_the_desktop_clients() {
        let b = boundary();
        assert!(b.starts_with("----codex-transcribe-"), "got {b}");
        assert!(!b.contains(char::is_whitespace), "a boundary must be one token");
    }
}
