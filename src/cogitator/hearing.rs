//! Hearing: a voice message goes to the transcribe ORGAN, and the text it returns
//! becomes the turn, as if it had been typed. Transcription is perception, and perception
//! is Octo's — the cogitator only hands the recording over and takes the words back. So
//! the organ's settings (language, how long recordings are cut) apply here too, and a
//! voice note of any length is heard.
//!
//! Hearing IS that organ: with no transcribe connector running (no manifest, no token)
//! Albert says, with regret, that he can't hear right now.

use std::{
    fs::{create_dir_all, write},
    path::Path,
    time::Duration,
};

use chrono::Utc;
use octo_core::{control::CANCEL_SCOPE_TAG, Blob, CogitatorContext, Envelope, EventKind};
use serde_json::{json, Value};
use tracing::info;

use super::AlbertCogitator;
use crate::status::StatusFeed;

/// The transcribe organ's command.
const TRANSCRIBE_RUN: &str = "transcribe.run";
/// How long a voice note may take to come back as text (a long one is cut on its pauses
/// and uploaded in parallel, ~15-30x real time).
const HEAR_TIMEOUT: Duration = Duration::from_secs(900);

impl AlbertCogitator {
    /// Automatic hearing runs inside the interruptible turn and uses its scope.
    pub(super) async fn hear(
        &self,
        incoming: &Envelope,
        path: &str,
        ctx: &CogitatorContext,
        scope: &str,
        feed: &StatusFeed,
    ) -> Result<String, String> {
        let organ = ctx.connectors().iter()
            .find(|c| c.capabilities.event_kinds_accept.iter().any(|k| k.as_str() == TRANSCRIBE_RUN))
            .map(|c| c.id.clone())
            .ok_or("I got your voice message, but I'm afraid I can't hear right now — there's no transcription connected. Could you write it instead?")?;
        let payload = json!({"path":path});
        let id = format!("auto-hear-{scope}");
        feed.start_external(
            &id,
            "dispatch_to_connector",
            json!({"target":organ.as_str(), "kind":TRANSCRIBE_RUN, "payload":payload}),
        );
        let request = Envelope::new(
            self.self_source.clone(),
            EventKind::from_static(TRANSCRIBE_RUN),
            payload,
        )
        .with_target(organ)
        .with_tag(CANCEL_SCOPE_TAG, scope);
        let result = match ctx.publish_and_await_response(request, HEAR_TIMEOUT).await {
            Ok(response) => response
                .payload_as::<Value>()
                .cloned()
                .unwrap_or(Value::Null),
            Err(error) => {
                let error = format!("Transcription did not return: {error}");
                feed.finish_external(&id, &json!({"error":error}).to_string());
                return Err(error);
            }
        };
        feed.finish_external(&id, &result.to_string());
        if let Some(error) = result.get("error").and_then(Value::as_str) {
            return Err(error.to_string());
        }
        let text = result
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if text.is_empty() {
            return Err("The voice message came through, but the transcript was empty.".into());
        }
        info!(
            chars = text.len(),
            "voice: heard through the transcribe organ"
        );
        Ok(
            match incoming
                .tags
                .get("caption")
                .filter(|caption| !caption.is_empty())
            {
                Some(caption) => format!("{text}\n\n(voice message caption: {caption})"),
                None => text.to_string(),
            },
        )
    }

    /// Put a recording the channel didn't save into the workspace inbox; returns its
    /// workspace-relative path.
    pub(super) fn keep_voice(&self, blob: &Blob) -> std::io::Result<String> {
        let ext = blob
            .filename()
            .and_then(|f| Path::new(f).extension())
            .and_then(|e| e.to_str())
            .unwrap_or("ogg");
        let rel = format!("inbox/voice-{}.{ext}", Utc::now().timestamp_millis());
        let full = self.config.code_workspace.join(&rel);
        if let Some(dir) = full.parent() {
            create_dir_all(dir)?;
        }
        write(&full, blob.bytes())?;
        Ok(rel)
    }
}
