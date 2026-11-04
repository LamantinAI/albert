//! Destination authorization is independent of the connector that originated work.
use std::time::{Duration, Instant};

use albert_core::{
    accounts::{AccountGrant, AccountRole},
    sessions::SessionContext,
};
use octo_connector_telegram::VerifiedBot;
use octo_core::Envelope;

use super::policy::SessionTelegramPolicy;

pub(super) struct DeliveryGrant {
    chat: i64,
    account: AccountGrant,
    expires: Instant,
}

impl SessionTelegramPolicy {
    pub(super) fn account_delivery_live(&self, envelope: &Envelope) -> bool {
        self.delivery_accounts.lock().ok().is_some_and(|grants| {
            grants
                .get(&envelope.id.to_string())
                .is_none_or(|grant| grant.expires > Instant::now() && grant.account.is_live())
        })
    }

    pub(super) async fn account_outbound(
        &self,
        bot: VerifiedBot,
        chat: i64,
        envelope: &Envelope,
    ) -> Result<Option<bool>, String> {
        let Some(accounts) = &self.accounts else {
            return Ok(None);
        };
        let Some(context) = envelope.context.get::<SessionContext>() else {
            return Ok(Some(false));
        };
        if !context.authorized().await {
            return Ok(Some(false));
        }
        if chat < 0 {
            let same_group = !context.is_private()
                && context.reply_route().connector() == bot.connector.as_str()
                && context.reply_route().channel() == chat.to_string();
            let owner = context
                .account()
                .is_some_and(|a| a.role == AccountRole::Owner);
            return Ok(Some(same_group || owner));
        }
        if chat == 0 {
            return Ok(Some(false));
        }
        // A saved route alone is historical attribution. Require today's active
        // binding for this exact bot and destination before sending private data.
        let Some(grant) = accounts
            .actor(bot.connector.as_str(), bot.bot_id, chat)
            .await
            .map_err(|_| "Telegram authorization is unavailable.".to_owned())?
        else {
            return Ok(Some(false));
        };
        if context
            .user_id()
            .is_none_or(|user| user.as_str() != grant.user_id)
            || !grant.valid().await
        {
            return Ok(Some(false));
        }
        let mut grants = self
            .delivery_accounts
            .lock()
            .map_err(|_| "Telegram authorization is unavailable.".to_owned())?;
        let now = Instant::now();
        grants.retain(|_, grant| grant.expires > now && grant.account.is_live());
        let key = envelope.id.to_string();
        if let Some(old) = grants.get(&key) {
            return Ok(Some(old.chat == chat && old.account.same_authority(&grant)));
        }
        if grants.len() >= self.service.settings().limits.max_account_scopes {
            return Ok(Some(false));
        }
        grants.insert(
            key,
            DeliveryGrant {
                chat,
                account: grant,
                expires: now
                    + Duration::from_secs(self.service.settings().limits.account_scope_ttl_seconds),
            },
        );
        Ok(Some(true))
    }
}
