use octo_core::{Blob, Envelope, InboundMessage};
use octo_history::PageRequest;
use rig::{completion::Message, message::UserContent, OneOrMany};

use super::{context::source_context, retention::temporary, AlbertCogitator, UserInput};
use crate::{
    composer::PreparedSkills,
    media::{
        attachment_note, decode, encode_prepared, encode_selected, workspace_note, Asset,
        MediaInput,
    },
    sessions::SessionContext,
};

impl AlbertCogitator {
    pub(super) async fn attachment_prompt(
        &self,
        incoming: &Envelope,
        input: &UserInput,
        channel: &str,
        accepted_id: Option<i64>,
    ) -> Message {
        let prompt = input.prompt_with_source(incoming);
        let assets = if let Some(media) = incoming.context.get::<MediaInput>() {
            media.assets.clone()
        } else if let Some(id) = accepted_id.filter(|_| !input.images.is_empty()) {
            self.history
                .journal_page(
                    channel,
                    PageRequest {
                        through_id: Some(id),
                        limit: 1,
                        ..PageRequest::default()
                    },
                )
                .await
                .ok()
                .and_then(|page| page.records.into_iter().find(|record| record.id == id))
                .and_then(|record| decode(&record.turn.content))
                .map(|record| record.attachments)
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        if assets.is_empty() {
            return prompt;
        }
        let Message::User { content } = prompt else {
            return prompt;
        };
        let mut parts: Vec<_> = content.into_iter().collect();
        parts.push(UserContent::text(
            assets.iter().map(attachment_note).collect::<String>(),
        ));
        Message::User {
            content: OneOrMany::many(parts).expect("attachment notes accompany a user message"),
        }
    }

    pub(super) fn web_media_input(&self, media: &MediaInput, incoming: &Envelope) -> UserInput {
        let temporary = temporary(incoming.context.get::<SessionContext>());
        let mut text = media.text.clone();
        let mut voice = None;
        let mut images = Vec::new();
        for (asset, blob) in media.assets.iter().zip(&media.blobs) {
            if blob.is_image() {
                images.push(blob.clone());
            } else if blob.is_audio() && !temporary {
                match self.keep_voice(blob) {
                    Ok(path) => voice = Some(path),
                    Err(_) => text.push_str("\nAudio attached, but transcription preparation failed. Do not claim to have listened to it."),
                }
            } else if !temporary {
                if let Some(file) = media
                    .workspace_files
                    .iter()
                    .find(|file| file.asset_id == asset.id)
                {
                    text.push_str(&workspace_note(asset, file));
                } else {
                    text.push_str(&format!("\nAttached file {:?} is retained for download, but no workspace copy is available in this chat. Do not claim to have read it.", asset.filename));
                }
            } else {
                text.push_str(&format!("\nAttached {:?} ({}). The file is available to the user, but file tools and audio/video analysis are unavailable in this temporary chat. Save the conversation and resend the attachment to enable workspace preparation.", asset.filename, asset.content_type));
            }
        }
        UserInput {
            text,
            images,
            seed: None,
            voice,
        }
    }
    pub(super) async fn media_transcript(
        &self,
        incoming: &Envelope,
        input: &UserInput,
    ) -> Result<String, String> {
        let Some(service) = &self.session_service else {
            return Ok(input.transcript_with_source(incoming));
        };
        if let Some(media) = incoming.context.get::<MediaInput>() {
            // The journal keeps the original caption, not generated workspace
            // hints. The live prompt may include derived perception context.
            let source = source_context(incoming);
            let text = if source.is_empty() {
                media.text.clone()
            } else {
                format!("{source}\n{}", media.text)
            };
            return Ok(encode_prepared(
                text,
                media.assets.clone(),
                incoming
                    .context
                    .get::<PreparedSkills>()
                    .map(PreparedSkills::stamps)
                    .unwrap_or_default(),
                media.workspace_files.clone(),
            ));
        }
        let blobs: Vec<Blob> = if let Some(blob) = incoming.payload_as::<Blob>() {
            vec![blob.clone()]
        } else if let Some(message) = incoming.payload_as::<InboundMessage>() {
            message.images.clone()
        } else {
            vec![]
        };
        let owner = incoming
            .context
            .get::<SessionContext>()
            .map(|s| s.session_id().as_str())
            .unwrap_or("native");
        let mut assets: Vec<Asset> = Vec::new();
        for blob in blobs {
            match service
                .media()
                .stage(
                    owner,
                    blob.bytes().to_vec(),
                    blob.content_type(),
                    blob.filename().unwrap_or("attachment"),
                )
                .await
            {
                Ok(asset) => assets.push(asset),
                Err(error) => return Err(error.to_string()),
            }
        }
        if assets.is_empty()
            && incoming
                .context
                .get::<PreparedSkills>()
                .is_none_or(PreparedSkills::is_empty)
        {
            return Ok(input.transcript_with_source(incoming));
        }
        Ok(encode_selected(
            input.transcript_with_source(incoming),
            assets,
            incoming
                .context
                .get::<PreparedSkills>()
                .map(PreparedSkills::stamps)
                .unwrap_or_default(),
        ))
    }
}
