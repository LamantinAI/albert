//! Private chat selection and persistent native command replay for session forks.
use octo_history::{HistoryError, RequestAlias, Session, SessionKind, SessionState};
use sqlx::{query, query_scalar, Sqlite, Transaction};

use super::{route_key, selected, SessionService};
use crate::{
    db::{
        error::DatabaseError,
        repo::route::{find_in, set},
    },
    sessions::{
        error::{SessionError, SessionResult},
        transport::NativeNavigation,
        SessionContext,
    },
};

const MAX_FORK_TITLE_BYTES: usize = 4096;

impl SessionService {
    /// Replay detection needs no selected session, including after the fork is deleted.
    pub async fn fork_replayed(&self, alias: &RequestAlias) -> SessionResult<bool> {
        let mut tx = self.history.begin_journal().await?;
        let seen = replayed_in(&mut tx, alias).await?;
        tx.commit().await.map_err(DatabaseError::from)?;
        Ok(seen)
    }

    /// Capture the committed source head and switch this private selector atomically.
    /// A repeated delivery leaves the user's current selector alone.
    pub async fn fork_private_native(
        &self,
        context: &SessionContext,
        title: &str,
        frame: &NativeNavigation,
    ) -> SessionResult<Option<SessionContext>> {
        context.require_private()?;
        super::native_routes::key(frame)?;
        if !context.authorized().await {
            return Err(SessionError::Denied);
        }
        let title = title.trim();
        if title.len() > MAX_FORK_TITLE_BYTES || title.chars().any(char::is_control) {
            return Err(SessionError::InvalidInput("use /fork [single-line title]"));
        }
        let _lifecycle = self.router.lifecycle_gate().write_owned().await;
        let mut tx = self.history.begin_journal().await?;
        if replayed_in(&mut tx, &frame.alias).await? {
            return Ok(None);
        }
        let parent = self
            .catalog_session_in(&mut tx, context.session_id())
            .await?;
        context.check(&parent)?;
        if parent.kind != SessionKind::Permanent || parent.state != SessionState::Active {
            return Err(SessionError::InvalidInput(
                "open a saved private chat before using /fork",
            ));
        }
        let key = route_key(context)?;
        let route = find_in(&mut tx, &key)
            .await?
            .ok_or(SessionError::NotFound)?;
        if route.session_id != parent.id.as_str()
            || Some(route.revision) != context.selector_revision()
        {
            return Err(DatabaseError::RevisionConflict.into());
        }
        if route.return_session_id.is_some() {
            return Err(SessionError::InvalidInput(
                "use /end before selecting another chat",
            ));
        }
        let title = if title.is_empty() {
            next_fork_title(&mut tx, &parent).await?
        } else {
            title.into()
        };
        let child = self
            .history
            .fork_session_in(&mut tx, &parent.id, None, title)
            .await?;
        let revision = set(&mut tx, &key, child.id.as_str(), None, Some(route.revision)).await?;
        let lease = self.lease_for_in(&mut tx, &child.id).await?;
        query("INSERT INTO albert_fork_receipts(namespace, request_key, request_digest) VALUES (?,?,?)")
            .bind(&frame.alias.namespace).bind(&frame.alias.idempotency_key).bind(&frame.alias.digest)
            .execute(&mut *tx).await.map_err(DatabaseError::from)?;
        tx.commit().await.map_err(DatabaseError::from)?;
        self.presentations
            .record(&key, &parent.id, route.revision, &child.id, revision);
        Ok(Some(selected(context, child, revision, lease, None)))
    }
}

async fn next_fork_title(
    tx: &mut Transaction<'_, Sqlite>,
    parent: &Session,
) -> SessionResult<String> {
    let number: i64 = query_scalar(
        "INSERT INTO albert_fork_name_sequences(session_id,last_number) VALUES (?,1)
        ON CONFLICT(session_id) DO UPDATE SET last_number=last_number+1 RETURNING last_number",
    )
    .bind(parent.id.as_str())
    .fetch_one(&mut **tx)
    .await
    .map_err(DatabaseError::from)?;
    let label: String = parent.title.chars().filter(|c| !c.is_control()).collect();
    let label = label.trim();
    let label = if label.is_empty() {
        "Unnamed chat"
    } else {
        label
    };
    let suffix = format!("-f{number}");
    let mut end = label.len().min(MAX_FORK_TITLE_BYTES - suffix.len());
    while !label.is_char_boundary(end) {
        end -= 1;
    }
    Ok(format!("{}{suffix}", &label[..end]))
}

async fn replayed_in(
    tx: &mut Transaction<'_, Sqlite>,
    alias: &RequestAlias,
) -> SessionResult<bool> {
    let digest: Option<String> = query_scalar(
        "SELECT request_digest FROM albert_fork_receipts WHERE namespace=? AND request_key=?",
    )
    .bind(&alias.namespace)
    .bind(&alias.idempotency_key)
    .fetch_optional(&mut **tx)
    .await
    .map_err(DatabaseError::from)?;
    match digest {
        Some(digest) if digest != alias.digest => Err(HistoryError::IdempotencyConflict.into()),
        digest => Ok(digest.is_some()),
    }
}
