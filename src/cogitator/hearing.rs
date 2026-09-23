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
    sync::Arc,
    time::Duration,
};

use chrono::Utc;
use octo_core::{Blob, CogitatorContext, Envelope, EventKind};
use serde_json::{json, Value};
use tracing::{info, warn};

use super::{channel_of, AlbertCogitator};

/// The transcribe organ's command.
const TRANSCRIBE_RUN: &str = "transcribe.run";
/// How long a voice note may take to come back as text (a long one is cut on its pauses
/// and uploaded in parallel, ~15-30x real time).
const HEAR_TIMEOUT: Duration = Duration::from_secs(900);

impl AlbertCogitator {
    /// Perceive a voice message through the transcribe organ and hand back the text.
    /// `None` means the voice went unheard *and the user has been told why* — silence is
    /// the one unacceptable answer to someone who just spoke.
    pub(super) async fn hear(
        self: &Arc<Self>,
        incoming: &Arc<Envelope>,
        blob: &Blob,
        ctx: &CogitatorContext,
    ) -> Option<String> {
        let decline = |reason: String| async move {
            self.emit_reply(incoming, reason.clone(), ctx).await;
            self.record(&channel_of(incoming), "(voice message)".into(), reason).await;
            None::<String>
        };

        let organ = ctx
            .connectors()
            .iter()
            .find(|c| c.capabilities.event_kinds_accept.iter().any(|k| k.as_str() == TRANSCRIBE_RUN))
            .map(|c| c.id.clone());
        let Some(organ) = organ else {
            return decline(
                "I got your voice message, but I'm afraid I can't hear right now — there's no \
                 transcription connected. Could you write it instead?"
                    .to_string(),
            )
            .await;
        };

        // The channel saved the recording to the workspace; the organ takes it from there.
        // A channel that didn't: keep it ourselves — the workspace is the cogitator's own.
        let path = match incoming.tags.get("workspace_path") {
            Some(path) => path.clone(),
            None => match self.keep_voice(blob) {
                Ok(path) => path,
                Err(e) => {
                    warn!(error = %e, "voice: could not keep the recording in the workspace");
                    return decline(format!("Couldn't keep the voice message to transcribe it: {e}")).await;
                }
            },
        };

        let request = Envelope::new(self.self_source.clone(), EventKind::from_static(TRANSCRIBE_RUN), json!({ "path": path }))
            .with_target(organ);
        let result = match ctx.publish_and_await_response(request, HEAR_TIMEOUT).await {
            Ok(response) => response.payload_as::<Value>().cloned().unwrap_or(Value::Null),
            Err(e) => {
                warn!(error = %e, "voice: the transcribe organ did not answer");
                return decline("Couldn't transcribe the voice message: the transcription didn't answer in time.".to_string())
                    .await;
            }
        };
        if let Some(e) = result.get("error").and_then(Value::as_str) {
            warn!(error = %e, "voice: transcription failed");
            return decline(format!("Couldn't transcribe the voice message: {e}")).await;
        }
        let text = result.get("text").and_then(Value::as_str).unwrap_or("").trim().to_string();
        if text.is_empty() {
            warn!("voice: empty transcript");
            return decline("The voice message came through but there's not a word in it — empty.".to_string()).await;
        }
        info!(
            chars = text.len(),
            secs = ?incoming.tags.get("duration_secs"),
            chunks = ?result.get("chunks"),
            "voice: heard through the transcribe organ"
        );
        // A caption (rare on voice, but possible) is context, not speech.
        match incoming.tags.get("caption").filter(|c| !c.is_empty()) {
            Some(caption) => Some(format!("{text}\n\n(voice message caption: {caption})")),
            None => Some(text),
        }
    }

    /// Put a recording the channel didn't save into the workspace inbox; returns its
    /// workspace-relative path.
    fn keep_voice(&self, blob: &Blob) -> std::io::Result<String> {
        let ext = blob.filename().and_then(|f| Path::new(f).extension()).and_then(|e| e.to_str()).unwrap_or("ogg");
        let rel = format!("inbox/voice-{}.{ext}", Utc::now().timestamp_millis());
        let full = self.config.code_workspace.join(&rel);
        if let Some(dir) = full.parent() {
            create_dir_all(dir)?;
        }
        write(&full, blob.bytes())?;
        Ok(rel)
    }
}
