use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::Utc;
use octo_core::{Blob, CogitatorContext, Envelope};
use rig::{
    completion::Message,
    message::{ImageMediaType, UserContent},
    OneOrMany,
};

use crate::history::{recent_actions, Turn, ACTION_MARKER};

/// One user turn as perceived: text, plus any images the connector downloaded
/// (a Telegram photo / image document, or every photo of an album; the caption
/// rides in `text`). Empty `images` is a plain text turn.
pub(super) struct UserInput {
    pub(super) text: String,
    pub(super) images: Vec<Blob>,
    /// What the model is actually given instead of `text`, when a skill command started
    /// the turn (the command + the skill's instructions). History keeps just `text`, so
    /// the instructions don't pile up in the transcript.
    pub(super) seed: Option<String>,
    pub(super) voice: Option<String>,
}

impl UserInput {
    /// The turn as the rig prompt message: plain text, or the image(s) + caption
    /// for a vision model (base64 travels fine through both the OpenRouter and the
    /// Codex Responses providers). An album sends every image in one message.
    pub(super) fn prompt(&self) -> Message {
        if self.images.is_empty() {
            return Message::user(self.seed.clone().unwrap_or_else(|| self.text.clone()));
        }
        let caption = if !self.text.trim().is_empty() {
            self.text.as_str()
        } else if self.images.len() == 1 {
            "The user sent this image with no caption — look at it and respond in the \
             context of the conversation."
        } else {
            "The user sent these images with no caption — look at them and respond in the \
             context of the conversation."
        };
        let mut content: Vec<UserContent> = self
            .images
            .iter()
            .map(|blob| {
                let b64 = STANDARD.encode(blob.bytes());
                UserContent::image_base64(b64, Some(media_type(blob.content_type())), None)
            })
            .collect();
        content.push(UserContent::text(caption));
        Message::User {
            content: OneOrMany::many(content).expect("at least one image plus the caption"),
        }
    }

    /// A text stand-in for logs and the history transcript (raw bytes don't
    /// belong in either).
    pub(super) fn transcript(&self) -> String {
        if let Some(path) = &self.voice {
            return format!("(voice recording: {path}) {}", self.text);
        }
        if self.images.is_empty() {
            return self.text.clone();
        }
        let noun = match self.images.as_slice() {
            [one] => format!("(sent an image, {})", one.content_type()),
            many => format!("(sent {} images)", many.len()),
        };
        if self.text.trim().is_empty() {
            noun
        } else {
            format!("{noun} {}", self.text)
        }
    }
}

/// Map a MIME content type onto rig's media-type enum (unknowns land on JPEG —
/// Telegram photos are JPEG re-encodes anyway).
pub(super) fn media_type(content_type: &str) -> ImageMediaType {
    match content_type {
        "image/png" => ImageMediaType::PNG,
        "image/gif" => ImageMediaType::GIF,
        "image/webp" => ImageMediaType::WEBP,
        "image/heic" => ImageMediaType::HEIC,
        "image/heif" => ImageMediaType::HEIF,
        _ => ImageMediaType::JPEG,
    }
}

pub(super) fn channel_of(env: &Envelope) -> String {
    env.channel
        .as_ref()
        .map(|c| c.as_str().to_string())
        .unwrap_or_default()
}

/// Fold this turn's action records into the assistant content we persist — so Albert's
/// transcript keeps what he DID (which tools, with which args, ok/err), not only what
/// he said. Appended to the STORED turn only, behind [`ACTION_MARKER`]; the reply the
/// user sees is untouched, and on reload the block is stripped from the assistant
/// message and surfaced via [`action_context`] instead (so it is never echoed to chat).
pub(super) fn with_action_log(answer: &str, actions: &[String]) -> String {
    if actions.is_empty() {
        return answer.to_string();
    }
    let mut s = String::from(answer);
    s.push_str(ACTION_MARKER);
    for a in actions {
        s.push_str("\n- ");
        s.push_str(a);
    }
    s
}

/// Render recent action logs as a preamble section — the agent's memory of what it
/// actually did, framed as system context (like the scratchpad) so it is read, not
/// repeated. Empty string when nothing recent, so it drops cleanly out of the format.
pub(super) fn action_context(turns: &[Turn]) -> String {
    match recent_actions(turns, 3) {
        Some(actions) => format!(
            "\n\nRECENT ACTIONS (system-recorded — the tools you actually ran on recent turns, \
             your ground truth for what you have already done; reference only, NEVER repeat these \
             lines in a reply):\n{actions}"
        ),
        None => String::new(),
    }
}

/// Current time as RFC3339 in the owner's configured timezone (offset form,
/// e.g. `…+03:00`), so the agent's notion of "now" — and thus "today" and any
/// reminder times it computes — is local rather than UTC.
pub(super) fn now_rfc3339(tz: &chrono_tz::Tz) -> String {
    Utc::now().with_timezone(tz).to_rfc3339()
}

/// Front-load the incoming envelope's provenance for the model — where the message
/// came from, so it can reply through the same channel and store it on reminders.
pub(super) fn incoming_context(env: &Envelope, channel: &str) -> String {
    format!(
        "Context — this message arrived via connector \"{}\", channel \"{}\". Reply through this \
         same connector/channel; when scheduling a reminder, put channel=\"{channel}\" and \
         reply_via=\"{}\" into the alarm payload.",
        env.source, channel, env.source
    )
}

/// Catalogue of connectors advertising a description (env-as-tools) for the
/// dispatch tool.
pub(super) fn catalog(ctx: &CogitatorContext) -> String {
    ctx.connectors()
        .iter()
        .filter_map(|c| {
            c.capabilities
                .description
                .as_ref()
                .map(|d| format!("- target \"{}\":\n{}", c.id, d))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn command_reply(text: &str) -> Option<String> {
    match text.trim() {
        "/start" => Some(
            "Hi! I'm Albert — an assistant on the Octo runtime with graph memory (kaeru) and a \
             scheduler. Say \"remind me …\" and I'll set a reminder and keep nudging you until \
             you say it's done. /help for more."
                .to_string(),
        ),
        _ => None,
    }
}
