use octo_history::HistoryError;
use thiserror::Error;

use crate::db::error::DatabaseError;

#[derive(Debug, Error)]
pub(crate) enum SessionError {
    #[error("session access denied")]
    Denied,
    #[error("session not found")]
    NotFound,
    #[error("this session has ended")]
    Ended,
    #[error("temporary chat expired after a restart; choose a chat or use /tmp or /end")]
    Expired,
    #[error("session lifecycle state is unavailable")]
    Unavailable,
    #[error("session selectors are available only in private channels")]
    GroupSelector,
    #[error("unsupported session host: {0}")]
    UnsupportedHost(String),
    #[error("invalid session input: {0}")]
    InvalidInput(&'static str),
    #[error(transparent)]
    History(#[from] HistoryError),
    #[error(transparent)]
    Database(#[from] DatabaseError),
}

pub(crate) type SessionResult<T> = Result<T, SessionError>;
