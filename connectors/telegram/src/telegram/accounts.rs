//! Account login is a native control path, before model input and journaling.
use albert_core::{
    accounts::{AccountAuthority, AccountGrant, AccountRole, LOGIN_TOKEN_PREFIX},
    commands::parse,
    sessions::SessionContext,
};
use octo_connector_telegram::{
    ActorRoleDecision, CommandMenuScope, IngressDecision, NativeFrame, PreflightDecision,
    PreflightFrame, Role, TelegramCommand, VerifiedActor, VerifiedBot,
};
use octo_core::{Envelope, EnvelopeContext};

use super::policy::SessionTelegramPolicy;

#[derive(Clone)]
pub(super) struct AccountInput {
    connector: String,
    bot: u64,
    actor: Option<i64>,
    chat: i64,
    update: u32,
    message: i32,
    pub(super) grant: Option<AccountGrant>,
    addressed: bool,
}

impl AccountInput {
    pub(super) fn matches(&self, frame: &NativeFrame) -> bool {
        self.connector == frame.connector.as_str()
            && self.bot == frame.bot_id
            && self.actor == frame.actor_id
            && self.chat == frame.chat_id
            && self.update == frame.update_id
            && self.message == frame.message_id
            && self.grant.as_ref().is_none_or(AccountGrant::is_live)
    }
}

fn role(grant: &AccountGrant) -> Role {
    if grant.role == AccountRole::Owner {
        Role::Owner
    } else {
        Role::Trusted
    }
}

fn consume(text: impl Into<String>, delete_message: bool) -> PreflightDecision {
    PreflightDecision::Consume {
        notice: Some(text.into()),
        delete_message,
    }
}

fn bot_credential(text: &str) -> bool {
    text.split_whitespace().any(|word| {
        let word = word.trim_matches(['`', '"', '\'']);
        word.split_once(':').is_some_and(|(id, key)| {
            (6..=20).contains(&id.len())
                && id.bytes().all(|b| b.is_ascii_digit())
                && key.len() >= 20
                && key
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
    })
}

impl SessionTelegramPolicy {
    pub fn with_accounts(mut self, accounts: AccountAuthority) -> Self {
        self.accounts = Some(accounts);
        self
    }

    pub(super) async fn account_bot_ready(&self, bot: VerifiedBot) -> Result<(), String> {
        if let Some(accounts) = &self.accounts {
            accounts
                .register_bot(bot.connector.as_str(), bot.bot_id, &bot.username)
                .await
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub(super) async fn account_preflight(
        &self,
        frame: PreflightFrame,
    ) -> Result<PreflightDecision, String> {
        let Some(accounts) = &self.accounts else {
            return Ok(PreflightDecision::ContinueLegacy);
        };
        let text = frame
            .text
            .as_deref()
            .or(frame.caption.as_deref())
            .unwrap_or("");
        let command = frame.command_text.as_deref().and_then(parse);
        let login = command.as_ref().is_some_and(|c| c.name == "login");
        if login || text.contains(LOGIN_TOKEN_PREFIX) {
            if !login
                || frame.chat_type != "private"
                || frame.forwarded
                || frame.actor_is_bot
                || frame.sender_chat_id.is_some()
            {
                return Ok(consume("Use /login <token> in your private chat with this bot. Create the token in Albert’s panel.", true));
            }
            let parts: Vec<_> = text.split_whitespace().collect();
            if parts.len() != 2 {
                return Ok(consume("Copy /login <token> from Telegram settings in Albert’s panel and send it here.", true));
            }
            let Some(actor) = frame.actor_id else {
                return Ok(consume("This message cannot be used to sign in.", true));
            };
            let response = match accounts
                .login(
                    frame.bot.connector.as_str(),
                    frame.bot.bot_id,
                    actor,
                    frame.chat_id,
                    parts[1],
                )
                .await
            {
                Ok(_) => "Signed in. You can continue your conversation with Albert.".to_owned(),
                Err(error) => error.to_string(),
            };
            return Ok(consume(response, true));
        }
        if bot_credential(text) {
            return Ok(consume(
                "Bot credentials belong in Albert’s panel, not in chat.",
                true,
            ));
        }
        let grant = if !frame.actor_is_bot && frame.sender_chat_id.is_none() {
            match frame.actor_id {
                Some(actor) => accounts
                    .actor(frame.bot.connector.as_str(), frame.bot.bot_id, actor)
                    .await
                    .map_err(|e| e.to_string())?,
                None => None,
            }
        } else {
            None
        };
        if command.as_ref().is_some_and(|c| c.name == "logout") {
            if frame.chat_type != "private" {
                return Ok(consume(
                    "Use /logout in your private chat with this bot.",
                    false,
                ));
            }
            if let Some(grant) = &grant {
                if let Some(binding) = &grant.binding {
                    accounts
                        .logout(
                            &binding.connector,
                            frame.bot.bot_id,
                            frame.actor_id.unwrap_or_default(),
                        )
                        .await
                        .map_err(|e| e.to_string())?;
                }
            }
            return Ok(consume(
                "Signed out of Telegram. Your account and conversation history are saved.",
                false,
            ));
        }
        let group = matches!(frame.chat_type.as_str(), "group" | "supergroup");
        if !group && frame.chat_type != "private" {
            return Ok(PreflightDecision::Consume {
                notice: None,
                delete_message: false,
            });
        }
        if frame.actor_is_bot {
            return Ok(PreflightDecision::Consume {
                notice: None,
                delete_message: false,
            });
        }
        if grant.is_none() && (!group || command.is_some()) {
            return Ok(consume("Sign in with /login <token> in a private chat. Create the token in Albert’s panel.", false));
        }
        let addressed = frame.addressed || command.is_some();
        if group && !addressed {
            return Ok(PreflightDecision::Consume {
                notice: None,
                delete_message: false,
            });
        }
        if let Some(command) = &command {
            if matches!(
                command.name.as_str(),
                "allow" | "deny" | "allowed" | "groupmode"
            ) {
                return Ok(consume("Telegram access now follows panel accounts. Manage accounts and connections in Albert’s panel.", false));
            }
            if group
                && matches!(
                    command.name.as_str(),
                    "sessions" | "new" | "open" | "rename" | "tmp" | "save" | "end"
                )
            {
                return Ok(consume(
                    "Personal conversations are available only in a private chat.",
                    false,
                ));
            }
            if grant.as_ref().is_some_and(|g| g.role != AccountRole::Owner)
                && !matches!(
                    command.name.as_str(),
                    "help"
                        | "start"
                        | "cancel"
                        | "compact"
                        | "chatinfo"
                        | "sessions"
                        | "new"
                        | "open"
                        | "rename"
                        | "tmp"
                        | "save"
                        | "end"
                )
            {
                return Ok(consume("This command requires the owner account.", false));
            }
        }
        if group && grant.is_none() && frame.text.is_none() {
            return Ok(consume(
                "Please address Albert with a text message, or sign in for more capabilities.",
                false,
            ));
        }
        let native_role = grant.as_ref().map(role).unwrap_or(Role::Guest);
        let stamp = AccountInput {
            connector: frame.bot.connector.to_string(),
            bot: frame.bot.bot_id,
            actor: frame.actor_id,
            chat: frame.chat_id,
            update: frame.update_id,
            message: frame.message_id,
            grant,
            addressed,
        };
        let mut context = EnvelopeContext::default();
        context.insert(stamp);
        Ok(PreflightDecision::Admit {
            role: native_role,
            context,
        })
    }

    pub(super) async fn account_role(
        &self,
        actor: VerifiedActor,
    ) -> Result<ActorRoleDecision, String> {
        let Some(accounts) = &self.accounts else {
            return Ok(ActorRoleDecision::ContinueLegacy);
        };
        if actor.actor_is_bot {
            return Ok(ActorRoleDecision::Role(None));
        }
        Ok(ActorRoleDecision::Role(
            accounts
                .actor(
                    actor.bot.connector.as_str(),
                    actor.bot.bot_id,
                    actor.actor_id,
                )
                .await
                .map_err(|e| e.to_string())?
                .as_ref()
                .map(role),
        ))
    }

    pub(super) async fn account_menu(
        &self,
        scope: CommandMenuScope,
    ) -> Result<Option<Vec<TelegramCommand>>, String> {
        let Some(accounts) = &self.accounts else {
            return Ok(None);
        };
        let (private, grant) = match scope {
            CommandMenuScope::PrivateChats => (true, None),
            CommandMenuScope::GroupChats => (false, None),
            CommandMenuScope::Actor(actor) => (
                actor.chat_type == "private",
                if actor.actor_is_bot {
                    None
                } else {
                    accounts
                        .actor(
                            actor.bot.connector.as_str(),
                            actor.bot.bot_id,
                            actor.actor_id,
                        )
                        .await
                        .map_err(|e| e.to_string())?
                },
            ),
        };
        let mut commands = Vec::new();
        let mut add = |command: &str, description: &str| {
            commands.push(TelegramCommand {
                command: command.into(),
                description: description.into(),
            })
        };
        let Some(grant) = grant else {
            if private {
                add("login", "Connect your Albert account");
            }
            return Ok(Some(commands));
        };
        add("help", "Available commands");
        if private || grant.role == AccountRole::Owner {
            add("cancel", "Stop the current task");
            add("compact", "Compact this conversation");
        }
        add("chatinfo", "Message source information");
        if private {
            for (name, description) in [
                ("sessions", "Choose a conversation"),
                ("new", "Start a new conversation"),
                ("open", "Open a conversation"),
                ("rename", "Rename this conversation"),
                ("tmp", "Start a temporary conversation"),
                ("save", "Save a temporary conversation"),
                ("end", "End a temporary conversation"),
                ("logout", "Disconnect Telegram"),
            ] {
                add(name, description);
            }
        }
        if grant.role == AccountRole::Owner {
            add("model", "Choose the agent model");
            add("restart", "Restart Albert");
        }
        Ok(Some(commands))
    }
}

pub(super) fn decorate(mut envelope: Envelope, input: &AccountInput) -> Envelope {
    if let Some(context) = envelope.context.get::<SessionContext>().cloned() {
        let context = match &input.grant {
            Some(grant) => context.with_account(grant.clone()),
            None => context.without_tools(),
        };
        envelope = envelope.with_context(context);
    }
    envelope.tags.remove("acl_admin");
    if let Some(metadata) = envelope.channel_metadata.as_mut() {
        metadata.tags.remove("acl_admin");
        if input.addressed {
            metadata.tags.insert("addressed".into(), "true".into());
        }
        metadata.tags.insert(
            "role".into(),
            input
                .grant
                .as_ref()
                .map(role)
                .unwrap_or(Role::Guest)
                .as_str()
                .into(),
        );
    }
    if input.addressed {
        envelope.tags.insert("addressed".into(), "true".into());
    }
    envelope
}

pub(super) fn decorate_decision(
    decision: IngressDecision,
    input: &AccountInput,
) -> IngressDecision {
    match decision {
        IngressDecision::Accept { envelope, scope } => IngressDecision::Accept {
            envelope: decorate(envelope, input),
            scope,
        },
        IngressDecision::Control {
            notice,
            scope,
            post_events,
            cleanup,
        } => IngressDecision::Control {
            notice,
            scope,
            post_events: post_events
                .into_iter()
                .map(|event| decorate(event, input))
                .collect(),
            cleanup,
        },
        other => other,
    }
}
