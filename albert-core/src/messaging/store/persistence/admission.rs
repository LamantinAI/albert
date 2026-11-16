use super::fingerprint;
use crate::messaging::store::MessageStore;
use crate::{
    messaging::contract::ContentRef, messaging::contract::DestinationSpec,
    messaging::contract::DisplayRoute, messaging::contract::MessageEnvelope,
    messaging::contract::ResolvedDestination, messaging::contract::MESSAGE_MARKER,
    messaging::policy::private_session, messaging::policy::RouteGuard, messaging::MessagingError,
    messaging::MessagingResult,
};
use octo_history::{AppendReceipt, AppendRequest, Session, SessionId, SessionJournal, Turn};
use serde::Serialize;
use serde_json::to_string;
use sqlx::{query, query_as, query_scalar, Sqlite, Transaction};
impl MessageStore {
    pub(in crate::messaging::store) async fn session_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        id: &SessionId,
        actor: &str,
    ) -> MessagingResult<Session> {
        let session = self.history.get_session_in(tx, id).await?;
        let marker: Option<(String, Option<String>)> =
            query_as("SELECT state,owner_user_id FROM albert_session_markers WHERE session_id=?")
                .bind(id.as_str())
                .fetch_optional(&mut **tx)
                .await?;
        let Some(session) = session else {
            if marker.as_ref().is_some_and(|(state, owner)| {
                state == "temporary" && owner.as_deref() == Some(actor)
            }) {
                return Err(MessagingError::SaveRequired);
            }
            return Err(MessagingError::Unavailable);
        };
        private_session(&session, actor)?;
        match marker.as_ref().map(|(state, _)| state.as_str()) {
            Some("temporary") => return Err(MessagingError::SaveRequired),
            Some("closed" | "expired") => return Err(MessagingError::Unavailable),
            _ => {}
        }
        Ok(session)
    }
    pub(in crate::messaging::store) async fn resolve_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        actor: &str,
        destination: &DestinationSpec,
        guard: &dyn RouteGuard,
    ) -> MessagingResult<ResolvedDestination> {
        let (id, display) = match destination {
            DestinationSpec::FixedSession { session, display } => {
                (session.clone(), display.clone())
            }
            DestinationSpec::ChannelActive { route } => (
                self.route_in(tx, actor, route, guard).await?,
                Some(route.clone()),
            ),
        };
        self.session_in(tx, &id, actor).await?;
        if let Some(route) = &display {
            self.verify_route_in(tx, actor, route, guard).await?;
        }
        Ok(ResolvedDestination {
            session: id,
            display,
        })
    }
    pub(in crate::messaging::store) async fn verify_route_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        actor: &str,
        route: &DisplayRoute,
        guard: &dyn RouteGuard,
    ) -> MessagingResult<()> {
        guard.check(actor, route)?;
        let bound: i64 = query_scalar("SELECT COUNT(*) FROM albert_private_session_routes WHERE connector_id=? AND channel_id=? AND user_id=?")
            .bind(&route.connector).bind(&route.channel).bind(actor).fetch_one(&mut **tx).await?;
        if bound != 1 {
            return Err(MessagingError::Denied);
        }
        Ok(())
    }
    pub(in crate::messaging::store) async fn route_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        actor: &str,
        route: &DisplayRoute,
        guard: &dyn RouteGuard,
    ) -> MessagingResult<SessionId> {
        guard.check(actor, route)?;
        let active: Option<String> = query_scalar("SELECT session_id FROM albert_private_session_routes WHERE connector_id=? AND channel_id=? AND user_id=?")
            .bind(&route.connector).bind(&route.channel).bind(actor).fetch_optional(&mut **tx).await?;
        let id = SessionId::parse(&active.ok_or(MessagingError::Denied)?)?;
        self.session_in(tx, &id, actor).await?;
        Ok(id)
    }
    pub(in crate::messaging::store) async fn capacity_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        recipient: &SessionId,
    ) -> MessagingResult<()> {
        let total: i64 = query_scalar("SELECT COUNT(*) FROM albert_messages")
            .fetch_one(&mut **tx)
            .await?;
        let outbox: i64 = query_scalar(
            "SELECT COUNT(*) FROM albert_message_outbox WHERE state IN ('pending','retry','started')",
        )
        .fetch_one(&mut **tx)
        .await?;
        if total >= self.limits.max_messages || outbox >= self.limits.max_outbox_pending {
            return Err(MessagingError::Limit);
        }
        self.inbox_capacity_in(tx, recipient).await
    }
    pub(in crate::messaging::store) async fn inbox_capacity_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        recipient: &SessionId,
    ) -> MessagingResult<()> {
        let pending: i64 = query_scalar("SELECT COUNT(*) FROM albert_message_inbox WHERE recipient_session=? AND work_state IN ('queued','running')")
            .bind(recipient.as_str()).fetch_one(&mut **tx).await?;
        if pending >= self.limits.max_inbox_pending {
            return Err(MessagingError::Limit);
        }
        Ok(())
    }
    pub(in crate::messaging::store) async fn admit_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        envelope: &MessageEnvelope,
        target: &ResolvedDestination,
    ) -> MessagingResult<()> {
        let exists: i64 = query_scalar(
            "SELECT COUNT(*) FROM albert_message_inbox WHERE message_id=? AND recipient_session=?",
        )
        .bind(envelope.id.as_str())
        .bind(target.session.as_str())
        .fetch_one(&mut **tx)
        .await?;
        if exists != 0 {
            return Ok(());
        }
        self.inbox_capacity_in(tx, &target.session).await?;
        let state = if envelope.intent.schedules_work() {
            "queued"
        } else {
            "passive"
        };
        query("INSERT INTO albert_message_inbox(message_id,recipient_session,work_state) VALUES(?,?,?)")
            .bind(envelope.id.as_str()).bind(target.session.as_str()).bind(state).execute(&mut **tx).await?;
        Ok(())
    }
    pub(in crate::messaging::store) async fn append_in<T: Serialize>(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        journal: &SessionJournal,
        id: &str,
        phase: &str,
        record: &T,
    ) -> MessagingResult<ContentRef> {
        let receipt = self
            .append_receipt_in(tx, journal, id, phase, record)
            .await?;
        Ok(ContentRef {
            journal: journal.clone(),
            record_id: receipt.through_id,
        })
    }
    pub(in crate::messaging::store) async fn append_receipt_in<T: Serialize>(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        journal: &SessionJournal,
        id: &str,
        phase: &str,
        record: &T,
    ) -> MessagingResult<AppendReceipt> {
        let request = AppendRequest {
            namespace: "albert.session-messaging.v1".into(),
            idempotency_key: format!("{id}:{phase}"),
            digest: fingerprint(record)?,
            turns: vec![Turn::assistant(format!(
                "{MESSAGE_MARKER}{}",
                to_string(record)?
            ))],
        };
        Ok(self
            .history
            .append_session_in(tx, journal, &request)
            .await?)
    }
}
