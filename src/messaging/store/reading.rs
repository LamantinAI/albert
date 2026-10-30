use octo_history::{PageRequest, SessionJournal, SessionStore};
use serde_json::from_value;
use sqlx::query_scalar;

use super::{decode_record, MessageRecord, MessageStore};
use crate::{
    messaging::{contract::MessageId, MessagingError, MessagingResult},
    sessions::SessionContext,
};

impl MessageStore {
    /// Load this exact bounded canonical request independently of the rolling
    /// model window/compact. Only its admitted recipient may use this seam.
    pub async fn received_record(
        &self,
        context: &SessionContext,
        id: &MessageId,
    ) -> MessagingResult<MessageRecord> {
        if !context.is_live() {
            return Err(MessagingError::Unavailable);
        }
        let _lease = self.lifecycle.read().await;
        let mut tx = self.history.begin_journal().await?;
        let actor = context.private_user_id()?.as_str();
        self.session_in(&mut tx, context.session_id(), actor)
            .await?;
        let record_id: Option<i64> = query_scalar("SELECT i.recipient_record FROM albert_message_inbox i JOIN albert_messages m USING(message_id) WHERE i.message_id=? AND i.recipient_session=? AND m.actor_id=? AND i.recipient_record IS NOT NULL")
            .bind(id.as_str()).bind(context.session_id().as_str()).bind(actor).fetch_optional(&mut *tx).await?;
        let record_id = record_id.ok_or(MessagingError::Unavailable)?;
        tx.commit().await?;
        let page = self
            .history
            .session_page(
                &SessionJournal::Root(context.session_id().clone()),
                PageRequest {
                    before_id: None,
                    through_id: Some(record_id),
                    limit: 1,
                },
            )
            .await?;
        let record = page
            .records
            .first()
            .filter(|record| record.id == record_id)
            .ok_or(MessagingError::Unavailable)?;
        Ok(from_value(decode_record(&record.turn.content)?)?)
    }
}
