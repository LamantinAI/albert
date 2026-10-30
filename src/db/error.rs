use std::result::Result as StdResult;

use sqlx::Error as SqlxError;
use thiserror::Error;

#[derive(Debug, Error)]
pub(crate) enum DatabaseError {
    #[error("internal database: {0}")]
    Sqlx(#[from] SqlxError),
    #[error("internal database migration {0} has a different checksum")]
    MigrationChecksum(i64),
    #[error("internal database schema version {0} is newer than this binary")]
    SchemaVersion(i64),
    #[error("internal database migration ledger is not a supported lineage")]
    MigrationLedger,
    #[error("external identity is already bound to a different user")]
    IdentityConflict,
    #[error("internal metadata changed; reload and retry")]
    RevisionConflict,
    #[error("invalid internal session marker state")]
    MarkerState,
}

pub(crate) type DatabaseResult<T> = StdResult<T, DatabaseError>;
