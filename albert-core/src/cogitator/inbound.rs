mod response;

use std::sync::Arc;

use octo_core::{Blob, CogitatorContext, Envelope, InboundMessage};
use tracing::warn;

use super::{compaction::CompactOptions, retention::temporary, AlbertCogitator, UserInput};
use crate::{
    accounts::AccountRole, acl::should_respond, composer::CompactRequest, composer::PreparedSkills,
    media::MediaInput, sessions::acceptance::SourceReceipt, sessions::actor::ActorToolPolicy,
    sessions::presentation::PresentationDetached, sessions::presentation::PresentationOpened,
    sessions::SessionContext, sessions::SessionEnded,
};

impl AlbertCogitator {
    pub(super) async fn handle(self: Arc<Self>, incoming: Arc<Envelope>, ctx: &CogitatorContext) {
        if incoming.kind.as_str() == "albert.presentation.opened" {
            if let Some(opened) = incoming.context.get::<PresentationOpened>() {
                self.open_presentation(&opened.0, ctx).await;
            }
            return;
        }
        if incoming.kind.as_str() == "albert.presentation.detached" {
            if let Some(detached) = incoming.context.get::<PresentationDetached>() {
                self.detach_presentation(detached, ctx).await;
            }
            return;
        }
        if incoming.kind.as_str() == "albert.session.closed" {
            if let Some(ended) = incoming.context.get::<SessionEnded>() {
                self.on_session_ended(&ended.session_id, ctx).await;
            }
            return;
        }
        if incoming.source == self.self_source {
            return; // never react to our own emissions
        }
        match incoming.kind.as_str() {
            "chat.message" => {
                if !should_respond(&incoming) {
                    return;
                }
                let receipt = incoming.context.get::<SourceReceipt>().cloned();
                let incoming = match self.admit_message(incoming, ctx).await {
                    Ok(incoming) => incoming,
                    Err(error) => {
                        if let Some(receipt) = receipt {
                            receipt.complete(Err("Session admission failed.".into()));
                        }
                        warn!(%error, "dialog session admission failed");
                        return;
                    }
                };
                if let Some(command) = incoming.context.get::<CompactRequest>() {
                    let permitted = incoming.source.as_str() == "web"
                        && incoming
                            .context
                            .get::<SessionContext>()
                            .is_some_and(|session| {
                                session.is_private() && session.account().is_some()
                            });
                    if !permitted {
                        if let Some(receipt) = incoming.context.get::<SourceReceipt>() {
                            receipt.complete(Err(
                                "This command requires an authenticated private session.".into(),
                            ));
                        }
                        return;
                    }
                    let input = UserInput {
                        text: format!("/compact {}", command.instructions),
                        images: Vec::new(),
                        seed: None,
                        voice: None,
                    };
                    let options = CompactOptions {
                        instructions: command.instructions.clone(),
                        dry_run: false,
                    };
                    match self.accept_control(&incoming, &input).await {
                        Ok(true) => self.clone().spawn_compact(incoming, ctx, options).await,
                        Ok(false) => {}
                        Err(error) => warn!(%error,"compact command was not accepted"),
                    }
                    return;
                }
                let selected = incoming
                    .context
                    .get::<PreparedSkills>()
                    .filter(|skills| !skills.is_empty());
                let selected = if let Some(selected) = selected {
                    let owner = incoming
                        .context
                        .get::<SessionContext>()
                        .is_some_and(|session| {
                            session.tool_policy() == ActorToolPolicy::ExistingHost
                                && session
                                    .account()
                                    .is_some_and(|account| account.role == AccountRole::Owner)
                        });
                    let prepared = if owner {
                        self.skills
                            .prepare_composer(selected.selections(), &self.config.sessions.composer)
                    } else {
                        Err("Only the owner may select installed skills.".into())
                    };
                    match prepared {
                        Ok(prepared) => Some(prepared),
                        Err(error) => {
                            if let Some(receipt) = incoming.context.get::<SourceReceipt>() {
                                receipt.complete(Err(format!(
                                    "Selected skill admission rejected: {error}"
                                )));
                            }
                            return;
                        }
                    }
                } else {
                    None
                };
                if temporary(incoming.context.get::<SessionContext>())
                    && incoming.payload_as::<Blob>().is_some_and(Blob::is_audio)
                {
                    self.emit_reply(&incoming, "Voice processing is unavailable in a temporary chat. Save this chat before sending voice messages.".into(), ctx).await;
                    return;
                }
                // A text payload is a normal turn; a Blob payload is media the
                // connector downloaded, its caption in tags — an image goes to the
                // model as-is, a voice message becomes text first (no Codex model
                // takes audio) and from there is an ordinary turn.
                let input = if let Some(media) = incoming.context.get::<MediaInput>() {
                    Some(self.web_media_input(media, &incoming))
                } else if let Some(text) = incoming.payload_as::<String>() {
                    Some(UserInput {
                        text: text.clone(),
                        images: Vec::new(),
                        seed: None,
                        voice: None,
                    })
                } else if let Some(blob) = incoming.payload_as::<Blob>().filter(|b| b.is_image()) {
                    Some(UserInput {
                        text: incoming.tags.get("caption").cloned().unwrap_or_default(),
                        images: vec![blob.clone()],
                        seed: None,
                        voice: None,
                    })
                } else if let Some(msg) = incoming.payload_as::<InboundMessage>() {
                    // A coalesced burst the connector grouped into one message: every
                    // photo of an album (shared media_group_id), or a forwarded run.
                    let images = msg
                        .images
                        .iter()
                        .filter(|b| b.is_image())
                        .cloned()
                        .collect();
                    Some(UserInput {
                        text: msg.text.clone().unwrap_or_default(),
                        images,
                        seed: None,
                        voice: None,
                    })
                } else if let Some(blob) = incoming.payload_as::<Blob>().filter(|b| b.is_audio()) {
                    let path = incoming
                        .tags
                        .get("workspace_path")
                        .cloned()
                        .map(Ok)
                        .unwrap_or_else(|| self.keep_voice(blob));
                    match path {
                        Ok(path) => Some(UserInput {
                            text: format!(
                                "Voice recording at {path}. {}",
                                incoming.tags.get("caption").cloned().unwrap_or_default()
                            ),
                            images: Vec::new(),
                            seed: None,
                            voice: Some(path),
                        }),
                        Err(error) => {
                            self.emit_reply(
                                &incoming,
                                format!("Couldn't save your voice message: {error}"),
                                ctx,
                            )
                            .await;
                            None
                        }
                    }
                } else {
                    None
                };
                if let Some(mut input) = input {
                    if let Some(selected) = selected {
                        input.seed = Some(selected.seed(&input.text));
                    }
                    self.respond(incoming, input, ctx).await;
                }
            }
            "alarm.fired" => self.on_alarm(incoming, ctx).await,
            _ => {}
        }
    }
}
