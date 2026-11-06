use octo_core::{Blob, Envelope, InboundMessage};

use super::{context::source_context, retention::temporary, AlbertCogitator, UserInput};
use crate::{
    composer::PreparedSkills,
    media::{encode_selected, save_input_file, Asset, MediaInput},
    sessions::SessionContext,
};

impl AlbertCogitator {
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
                if let Ok(path) =
                    save_input_file(&self.config.code_workspace, &asset.id, blob.bytes())
                {
                    text.push_str(&format!("\nAttached file {:?} is available at workspace path {:?}. This file is not native visual input.", asset.filename, path));
                }
            } else {
                text.push_str(&format!("\nAttached {:?} ({}). Playback is available to the user; automatic audio/video analysis is unavailable in this temporary chat.", asset.filename, asset.content_type));
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
            return Ok(encode_selected(
                text,
                media.assets.clone(),
                incoming
                    .context
                    .get::<PreparedSkills>()
                    .map(PreparedSkills::stamps)
                    .unwrap_or_default(),
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
