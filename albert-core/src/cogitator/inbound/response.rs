use std::sync::Arc;

use octo_core::{CogitatorContext, Envelope};
use tracing::{info, warn};

use crate::cogitator::{
    command_reply, compaction::CompactOptions, turn_key, AlbertCogitator, UserInput,
};
use crate::{
    acl::command as acl_command, acl::is_acl_admin, acl::is_owner, acl::tag,
    commands::account_help, commands::help, commands::parse, commands::seed,
    sessions::SessionContext,
};

impl AlbertCogitator {
    /// A user message → a normal agent turn with memory + scheduler + scratchpad tools.
    pub(in crate::cogitator) async fn respond(
        self: &Arc<Self>,
        incoming: Arc<Envelope>,
        input: UserInput,
        ctx: &CogitatorContext,
    ) {
        // Reflexes fire on text-only turns: instant, no LLM.
        if incoming.source.as_str() != "web"
            && input.images.is_empty()
            && tag(&incoming, "forwarded") != Some("true")
        {
            let owner = is_owner(&incoming);
            let personal_control = if let Some(session) = incoming.context.get::<SessionContext>() {
                session.is_private() && session.account().is_some() && session.authorized().await
            } else {
                false
            };
            let command_text = tag(&incoming, "command_text").unwrap_or(&input.text);
            let invocation = parse(command_text);
            let command = invocation
                .as_ref()
                .map(|cmd| cmd.name.as_str())
                .unwrap_or("");

            if matches!(
                command,
                "compact"
                    | "model"
                    | "cancel"
                    | "restart"
                    | "help"
                    | "start"
                    | "allow"
                    | "deny"
                    | "allowed"
            ) {
                match self.accept_control(&incoming, &input).await {
                    Ok(true) => {}
                    Ok(false) => return,
                    Err(error) => {
                        warn!(%error, "control input was not accepted");
                        return;
                    }
                }
            }

            if command == "compact" {
                if !owner && !personal_control {
                    self.emit_reply(
                        &incoming,
                        "Only the owner can compact this conversation.".into(),
                        ctx,
                    )
                    .await;
                } else {
                    let args = invocation.as_ref().map(|c| c.args).unwrap_or("");
                    match CompactOptions::parse(args) {
                        Ok(options) => self.clone().spawn_compact(incoming, ctx, options).await,
                        Err(error) => self.emit_reply(&incoming, error, ctx).await,
                    }
                }
                return;
            }

            if command == "model" {
                let args = invocation.as_ref().map(|c| c.args).unwrap_or("");
                let reply = self.models.command(args, owner);
                self.finish_reflex(&incoming, input.text, reply, ctx, false)
                    .await;
                return;
            }

            if !owner && (command == "restart" || (command == "cancel" && !personal_control)) {
                self.emit_reply(
                    &incoming,
                    "Only the owner can use this command.".into(),
                    ctx,
                )
                .await;
                return;
            }
            // The owner may stop group work; registered users control only
            // their own private session, never another participant's group task.
            if (owner || personal_control) && command == "cancel" {
                let stopped = self.cancel_channel(&turn_key(&incoming), ctx).await;
                let msg = if stopped {
                    "Stopped."
                } else {
                    "Nothing to stop right now."
                };
                self.finish_reflex(&incoming, input.text, msg.into(), ctx, false)
                    .await;
                return;
            }

            // Owner-only /restart: a deterministic reflex onto the same control signal
            // the model's `restart` tool uses — force a process restart, no LLM turn.
            if owner && command == "restart" {
                if self
                    .finish_reflex(
                        &incoming,
                        input.text,
                        "Restarting — back in a couple of seconds.".into(),
                        ctx,
                        false,
                    )
                    .await
                {
                    self.apply_restart("process".to_string(), ctx).await;
                }
                return;
            }

            // /help lists the system commands and the skills' commands this user may run.
            if command == "help" {
                let skills = if incoming
                    .context
                    .get::<SessionContext>()
                    .is_some_and(SessionContext::chat_only)
                {
                    Vec::new()
                } else {
                    self.skills.commands()
                };
                let reply = match incoming
                    .context
                    .get::<SessionContext>()
                    .filter(|s| s.account().is_some())
                {
                    Some(session) => account_help(&skills, owner, session.is_private()),
                    None => help(&skills, owner, is_acl_admin(&incoming)),
                };
                self.finish_reflex(&incoming, input.text, reply, ctx, false)
                    .await;
                return;
            }

            if let Some(canned) = command_reply(if command == "start" {
                "/start"
            } else {
                &input.text
            }) {
                self.finish_reflex(&incoming, input.text, canned, ctx, false)
                    .await;
                return;
            }

            // Reflex: owner-only ACL admin, deterministic (out of the LLM).
            if let Some(reply) = acl_command(&self.self_source, command_text, &incoming, ctx).await
            {
                self.finish_reflex(&incoming, input.text, reply, ctx, true)
                    .await;
                return;
            }

            // A skill's command: an ordinary turn, seeded with that skill's instructions.
            // An unknown `/word` falls through and reaches the agent as text.
            let invoked = parse(command_text)
                .filter(|_| {
                    !incoming
                        .context
                        .get::<SessionContext>()
                        .is_some_and(SessionContext::chat_only)
                })
                .and_then(|inv| {
                    self.skills
                        .command(&inv.name)
                        .map(|c| (c, inv.args.to_string()))
                });
            if let Some((command, args)) = invoked {
                if command.owner && !owner {
                    let reply = format!("/{} is for the owner only.", command.name);
                    self.finish_reflex(&incoming, input.text, reply, ctx, false)
                        .await;
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
                        self.finish_reflex(&incoming, input.text, reply, ctx, false)
                            .await;
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
