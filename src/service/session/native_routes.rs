//! Content-free navigation fences. This is a bounded policy for ordinary
//! Telegram Message dates with Update IDs breaking ties, not a claim that
//! Telegram guarantees a total order for every possible update type.

use std::{collections::HashMap, sync::Mutex};

use octo_connector_telegram::NativeFrame;
use octo_history::SessionId;
use sqlx::{Sqlite, Transaction};

use super::SessionService;
use crate::{
    db::{
        entity::{Excursion, ExcursionKey, NativePosition},
        error::DatabaseError,
        repo::excursion::{close_in, insert_in, list_in},
    },
    sessions::{
        cleanup_scope_for,
        error::{SessionError, SessionResult},
        SessionContext,
    },
};

#[derive(Default)]
pub(super) struct NativeSeen {
    positions: Mutex<HashMap<String, NativePosition>>,
}

#[derive(Clone, Debug)]
pub(crate) enum NativeRoute {
    Current,
    Pinned {
        session_id: SessionId,
        closed: bool,
        discarded: bool,
    },
    ControlReplay,
}

pub(super) fn key(frame: &NativeFrame) -> SessionResult<ExcursionKey> {
    if frame.bot_id == 0 || frame.message_id <= 0 || frame.date <= 0 || frame.chat_type != "private"
    {
        return Err(SessionError::InvalidInput(
            "unsupported native navigation event",
        ));
    }
    let actor = frame.actor_id.ok_or(SessionError::Denied)?;
    Ok(ExcursionKey {
        connector_id: frame.connector.to_string(),
        bot_id: frame.bot_id.to_string(),
        chat_id: frame.chat_id.to_string(),
        actor_id: actor.to_string(),
    })
}

pub(super) fn position(frame: &NativeFrame) -> NativePosition {
    NativePosition {
        date: frame.date,
        update_id: i64::from(frame.update_id),
    }
}

impl SessionService {
    pub async fn resolve_native_frame(&self, frame: &NativeFrame) -> SessionResult<NativeRoute> {
        let key = key(frame)?;
        let point = position(frame);
        let mut tx = self.history.begin_journal().await?;
        let fences = list_in(&mut tx, &key).await?;
        tx.commit().await.map_err(DatabaseError::from)?;
        if fences.iter().any(|fence| {
            (fence.start == point && fence.start_message_id == i64::from(frame.message_id))
                || (fence.end == Some(point)
                    && fence.end_message_id == Some(i64::from(frame.message_id)))
        }) {
            return Ok(NativeRoute::ControlReplay);
        }
        let matches: Vec<_> = fences
            .iter()
            .filter(|fence| point >= fence.start && fence.end.is_none_or(|end| point <= end))
            .collect();
        if matches.len() > 1 {
            return Err(SessionError::InvalidInput(
                "ambiguous native navigation boundary",
            ));
        }
        if let Some(fence) = matches.first() {
            return Ok(NativeRoute::Pinned {
                session_id: SessionId::parse(&fence.session_id)?,
                closed: fence.end.is_some(),
                discarded: fence.disposition == "discarded",
            });
        }
        // A delayed frame from before an active /tmp belongs to its return
        // session, rather than the newly selected temporary context.
        let open: Vec<_> = fences.iter().filter(|fence| fence.end.is_none()).collect();
        if open.len() > 1 {
            return Err(SessionError::InvalidInput("ambiguous active navigation"));
        }
        if let Some(fence) = open.first().filter(|fence| point < fence.start) {
            return Ok(NativeRoute::Pinned {
                session_id: SessionId::parse(&fence.return_session_id)?,
                closed: false,
                discarded: false,
            });
        }
        Ok(NativeRoute::Current)
    }

    pub fn note_native_frame(
        &self,
        context: &SessionContext,
        frame: &NativeFrame,
    ) -> SessionResult<()> {
        if context.cleanup_scope().is_none() {
            return Ok(());
        }
        let key = cleanup_scope_for(context.reply_route(), context.session_id());
        let mut seen = self
            .native_seen
            .positions
            .lock()
            .map_err(|_| SessionError::Unavailable)?;
        let point = position(frame);
        seen.entry(key)
            .and_modify(|max| *max = (*max).max(point))
            .or_insert(point);
        Ok(())
    }

    pub(super) async fn start_native_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        context: &SessionContext,
        id: &SessionId,
        return_id: &str,
        frame: Option<&NativeFrame>,
    ) -> SessionResult<()> {
        let Some(frame) = frame else {
            return Ok(());
        };
        let key = key(frame)?;
        let point = position(frame);
        let fences = list_in(tx, &key).await?;
        for fence in &fences {
            if fence.start == point && fence.start_message_id == i64::from(frame.message_id) {
                return Err(SessionError::InvalidInput(
                    "this navigation command was already handled",
                ));
            }
            if fence.end.is_none() {
                if fence.disposition == "active" {
                    return Err(SessionError::InvalidInput(
                        "save or end the current temporary chat first",
                    ));
                }
                close_in(tx, fence, point, i64::from(frame.message_id)).await?;
            }
        }
        insert_in(
            tx,
            &Excursion {
                key,
                user_id: context.private_user_id()?.as_str().into(),
                session_id: id.to_string(),
                return_session_id: return_id.into(),
                scope_epoch: self.runtime_epoch.clone(),
                start: point,
                start_message_id: i64::from(frame.message_id),
                end: None,
                end_message_id: None,
                disposition: "active".into(),
                revision: 1,
            },
        )
        .await?;
        Ok(())
    }

    pub(super) async fn close_native_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        context: &SessionContext,
        id: &SessionId,
        frame: Option<&NativeFrame>,
    ) -> SessionResult<()> {
        let Some(frame) = frame else {
            return Ok(());
        };
        let point = position(frame);
        let scope = cleanup_scope_for(context.reply_route(), id);
        if self
            .native_seen
            .positions
            .lock()
            .map_err(|_| SessionError::Unavailable)?
            .get(&scope)
            .is_some_and(|max| point < *max)
        {
            return Err(SessionError::InvalidInput(
                "native navigation is out of order; retry explicitly",
            ));
        }
        for fence in list_in(tx, &key(frame)?).await? {
            if fence.session_id == id.as_str() && fence.end.is_none() {
                close_in(tx, &fence, point, i64::from(frame.message_id)).await?;
            }
        }
        Ok(())
    }
}
