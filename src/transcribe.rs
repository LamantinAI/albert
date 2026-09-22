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
//!
//! The upload itself is octo's (`octo_connector_transcribe::transcribe`) — the same code
//! path the `transcribe` connector serves to the agent. What stays here is Albert's policy
//! for hearing a voice message inline.

/// Longest voice message transcribed inline, in seconds.
///
/// The endpoint accepts roughly 23 minutes; past that it answers `500`. Worse, a
/// long-but-accepted upload can come back `200` with the transcript **silently cut
/// off mid-sentence**, so the ceiling here is deliberately below the failure point
/// rather than at it. Longer recordings belong to the transcription skill, which
/// splits them on silence and stitches the pieces back together.
pub const MAX_INLINE_SECS: u32 = 20 * 60;
