use sqlx::{query, query_scalar, Sqlite, Transaction};

use crate::db::error::{DatabaseError, DatabaseResult};

pub(crate) async fn find_in(
    tx: &mut Transaction<'_, Sqlite>,
    connector: &str,
    actor: &str,
) -> DatabaseResult<Option<String>> {
    Ok(query_scalar(
        "SELECT user_id FROM albert_identity_bindings \
         WHERE connector_id = ? AND external_actor_id = ?",
    )
    .bind(connector)
    .bind(actor)
    .fetch_optional(&mut **tx)
    .await?)
}

/// The service verifies the identity before calling this repository. A binding
/// cannot be silently reassigned, including when two admissions race.
pub(crate) async fn bind(
    tx: &mut Transaction<'_, Sqlite>,
    connector: &str,
    actor: &str,
    user: &str,
) -> DatabaseResult<()> {
    query("INSERT INTO albert_users (id) VALUES (?) ON CONFLICT (id) DO NOTHING")
        .bind(user)
        .execute(&mut **tx)
        .await?;
    query(
        "INSERT INTO albert_identity_bindings (connector_id, external_actor_id, user_id) \
         VALUES (?, ?, ?) ON CONFLICT (connector_id, external_actor_id) DO NOTHING",
    )
    .bind(connector)
    .bind(actor)
    .bind(user)
    .execute(&mut **tx)
    .await?;
    let bound: String = query_scalar(
        "SELECT user_id FROM albert_identity_bindings \
         WHERE connector_id = ? AND external_actor_id = ?",
    )
    .bind(connector)
    .bind(actor)
    .fetch_one(&mut **tx)
    .await?;
    if bound != user {
        return Err(DatabaseError::IdentityConflict);
    }
    Ok(())
}
