//! Host-owned session addressing, durable control metadata and canonical journal
//! transactions. No runtime delivery or model invocation is performed here.
pub(crate) mod contract;
pub(crate) mod policy;
pub(crate) mod settings;
pub(crate) mod store;
pub(crate) mod transport;

use octo_history::HistoryError;
use serde_json::Error as JsonError;
use sqlx::Error as SqlError;
use thiserror::Error;

use crate::sessions::error::SessionError;

#[derive(Debug, Error)]
pub(crate) enum MessagingError {
    #[error("invalid messaging input: {0}")]
    Invalid(&'static str),
    #[error("session messaging access denied")]
    Denied,
    #[error("save the temporary session before exporting messages")]
    SaveRequired,
    #[error("session or destination is unavailable")]
    Unavailable,
    #[error("messaging queue or payload limit reached")]
    Limit,
    #[error("message identity or state conflict")]
    Conflict,
    #[error(transparent)]
    History(#[from] HistoryError),
    #[error(transparent)]
    Database(#[from] SqlError),
    #[error(transparent)]
    Json(#[from] JsonError),
    #[error(transparent)]
    Session(#[from] SessionError),
}

pub(crate) type MessagingResult<T> = Result<T, MessagingError>;
