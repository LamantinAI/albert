use std::sync::Arc;

use octo_core::{Blob, CogitatorContext, Envelope, InboundMessage};
use tracing::info;

use super::{channel_of, command_reply, turn_key, AlbertCogitator, UserInput};
use crate::{
    acl::{command as acl_command, is_acl_admin, is_owner, should_respond, tag},
    commands::{help, parse, seed},
};

impl AlbertCogitator {
    pub(super) async fn handle(self: Arc<Self>, incoming: Arc<Envelope>, ctx: &CogitatorContext) {
        if incoming.source == self.self_source {
            return; // never react to our own emissions
        }
        match incoming.kind.as_str() {
            "chat.message" => {
                if !should_respond(&incoming) {
                    return;
                }
                // A text payload is a normal turn; a Blob payload is media the
                // connector downloaded, its caption in tags — an image goes to the
                // model as-is, a voice message becomes text first (no Codex model
                // takes audio) and from there is an ordinary turn.
                let input = if let Some(text) = incoming.payload_as::<String>() {
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
                if let Some(input) = input {
                    self.respond(incoming, input, ctx).await;
                }
            }
            "alarm.fired" => self.on_alarm(incoming, ctx).await,
            _ => {}
        }
    }

    /// A user message → a normal agent turn with memory + scheduler + scratchpad tools.
    pub(super) async fn respond(
        self: &Arc<Self>,
        incoming: Arc<Envelope>,
        input: UserInput,
        ctx: &CogitatorContext,
    ) {
        let channel_key = channel_of(&incoming);

        // Reflexes fire on text-only turns: instant, no LLM.
        if input.images.is_empty() && tag(&incoming, "forwarded") != Some("true") {
            let owner = is_owner(&incoming);
            let command_text = tag(&incoming, "command_text").unwrap_or(&input.text);
            let invocation = parse(command_text);
            let command = invocation
                .as_ref()
                .map(|cmd| cmd.name.as_str())
                .unwrap_or("");

            if command == "model" {
                let args = invocation.as_ref().map(|c| c.args).unwrap_or("");
                let reply = self.models.command(args, owner);
                self.emit_reply(&incoming, reply, ctx).await;
                return;
            }

            if !owner && matches!(command, "cancel" | "restart") {
                self.emit_reply(
                    &incoming,
                    "Only the owner can use this command.".into(),
                    ctx,
                )
                .await;
                return;
            }
            // Owner-only /cancel: stop this channel's in-flight turn — abort its task AND
            // cancel the connector work it started (forkd scripts) — with no successor.
            // Non-owners can't halt Albert, so for them it falls through as ordinary text.
            if owner && command == "cancel" {
                let stopped = self.cancel_channel(&turn_key(&incoming), ctx).await;
                let msg = if stopped {
                    "Stopped."
                } else {
                    "Nothing to stop right now."
                };
                self.emit_reply(&incoming, msg.to_string(), ctx).await;
                self.record(&channel_key, input.text, "(cancel)".into())
                    .await;
                return;
            }

            // Owner-only /restart: a deterministic reflex onto the same control signal
            // the model's `restart` tool uses — force a process restart, no LLM turn.
            if owner && command == "restart" {
                self.emit_reply(
                    &incoming,
                    "Restarting — back in a couple of seconds.".to_string(),
                    ctx,
                )
                .await;
                self.record(&channel_key, input.text, "(restart)".into())
                    .await;
                self.apply_restart("process".to_string(), ctx).await;
                return;
            }

            // /help lists the system commands and the skills' commands this user may run.
            if command == "help" {
                let reply = help(&self.skills.commands(), owner, is_acl_admin(&incoming));
                self.emit_reply(&incoming, reply, ctx).await;
                self.record(&channel_key, input.text, "(help)".into()).await;
                return;
            }

            if let Some(canned) = command_reply(if command == "start" {
                "/start"
            } else {
                &input.text
            }) {
                self.emit_reply(&incoming, canned.clone(), ctx).await;
                self.record(&channel_key, input.text, "(reflex reply)".into())
                    .await;
                return;
            }

            // Reflex: owner-only ACL admin, deterministic (out of the LLM).
            if let Some(reply) = acl_command(&self.self_source, command_text, &incoming, ctx).await
            {
                self.emit_control_reply(&incoming, reply, ctx).await;
                self.record(&channel_key, input.text, "(acl command)".into())
                    .await;
                return;
            }

            // A skill's command: an ordinary turn, seeded with that skill's instructions.
            // An unknown `/word` falls through and reaches the agent as text.
            let invoked = parse(command_text).and_then(|inv| {
                self.skills
                    .command(&inv.name)
                    .map(|c| (c, inv.args.to_string()))
            });
            if let Some((command, args)) = invoked {
                if command.owner && !owner {
                    let reply = format!("/{} is for the owner only.", command.name);
                    self.emit_reply(&incoming, reply.clone(), ctx).await;
                    self.record(&channel_key, input.text, reply).await;
                    return;
                }
                match self.skills.instructions(&command.skill) {
                    Ok((instructions, files)) => {
                        info!(command = %command.name, skill = %command.skill, "command: running a skill");
                        let seed = Some(seed(&command, &args, &instructions, &files));
                        let input = UserInput {
                            text: input.text,
                            images: Vec::new(),
                            seed,
                            voice: None,
                        };
                        self.clone().spawn_turn(incoming, input, ctx).await;
                    }
                    Err(e) => {
                        let reply = format!("/{} couldn't load its skill: {e}", command.name);
                        self.emit_reply(&incoming, reply.clone(), ctx).await;
                        self.record(&channel_key, input.text, reply).await;
                    }
                }
                return;
            }
        }

        // The actual agent turn runs as its own task, so the perceive loop stays free to
        // receive the next message (and a /cancel) while it runs. A new message on the
        // channel interrupts and continues the working context already running there.
        self.clone().spawn_turn(incoming, input, ctx).await;
    }
}
