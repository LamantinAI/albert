//! Shared-prefix forks for web resources and native private selectors.
use octo_history::{
    HistoryError, RequestAlias, Session, SessionAccess, SessionId, SessionKind, SessionState,
};
use sha2::{Digest, Sha256};
use sqlx::{query, query_as, query_scalar, Sqlite, Transaction};

use super::{
    route_key, selected,
    web::{verify_actor, WebActor},
    SessionService,
};
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
        require_saved_chat(&parent)?;
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
        let child = self.create_fork_in(&mut tx, &parent, title).await?;
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

    /// Fork an explicit web session without changing a native channel selector.
    pub async fn web_fork(
        &self,
        actor: &WebActor,
        source: &SessionId,
        request_id: &str,
    ) -> SessionResult<Session> {
        let _lifecycle = self.router.lifecycle_gate().write_owned().await;
        let mut tx = self.history.begin_journal().await?;
        verify_actor(&mut tx, actor).await?;
        let parent = self.catalog_session_in(&mut tx, source).await?;
        let access = SessionAccess::Private {
            owner: actor.user_id.as_str().into(),
        };
        if parent.access != access {
            return Err(SessionError::Denied);
        }
        let namespace = format!("web:fork:{}", actor.account_id);
        let digest = format!("{:x}", Sha256::digest(source.as_str().as_bytes()));
        let receipt: Option<(String, Option<String>)> = query_as(
            "SELECT request_digest, fork_session_id FROM albert_fork_receipts WHERE namespace=? AND request_key=?",
        )
        .bind(&namespace)
        .bind(request_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(DatabaseError::from)?;
        let child = if let Some((stored_digest, child_id)) = receipt {
            if stored_digest != digest {
                return Err(HistoryError::IdempotencyConflict.into());
            }
            let id = child_id.ok_or(SessionError::NotFound)?;
            let child = self
                .catalog_session_in(&mut tx, &SessionId::parse(&id)?)
                .await?;
            if child.access != access {
                return Err(SessionError::Denied);
            }
            child
        } else {
            require_saved_chat(&parent)?;
            let child = self.create_fork_in(&mut tx, &parent, "").await?;
            query("INSERT INTO albert_fork_receipts(namespace,request_key,request_digest,fork_session_id) VALUES (?,?,?,?)")
                .bind(&namespace)
                .bind(request_id)
                .bind(&digest)
                .bind(child.id.as_str())
                .execute(&mut *tx).await.map_err(DatabaseError::from)?;
            child
        };
        tx.commit().await.map_err(DatabaseError::from)?;
        Ok(child)
    }

    async fn create_fork_in(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        parent: &Session,
        title: &str,
    ) -> SessionResult<Session> {
        let title = if title.is_empty() {
            next_fork_title(tx, parent).await?
        } else {
            title.into()
        };
        Ok(self
            .history
            .fork_session_in(tx, &parent.id, None, title)
            .await?)
    }
}

fn require_saved_chat(session: &Session) -> SessionResult<()> {
    if session.kind != SessionKind::Permanent || session.state != SessionState::Active {
        return Err(SessionError::InvalidInput(
            "open a saved private chat before using /fork",
        ));
    }
    Ok(())
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
