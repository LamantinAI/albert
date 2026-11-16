use super::role;
use crate::telegram::policy::SessionTelegramPolicy;
use albert_core::accounts::{AccountAuthority, AccountRole};
use octo_connector_telegram::{
    ActorRoleDecision, CommandMenuScope, TelegramCommand, VerifiedActor, VerifiedBot,
};
impl SessionTelegramPolicy {
    pub fn with_accounts(mut self, accounts: AccountAuthority) -> Self {
        self.accounts = Some(accounts);
        self
    }
    pub(in crate::telegram) async fn account_bot_ready(
        &self,
        bot: VerifiedBot,
    ) -> Result<(), String> {
        if let Some(accounts) = &self.accounts {
            accounts
                .register_bot(bot.connector.as_str(), bot.bot_id, &bot.username)
                .await
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    pub(in crate::telegram) async fn account_role(
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
    pub(in crate::telegram) async fn account_menu(
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
