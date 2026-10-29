use sqlx::{query, query_as, query_scalar, Sqlite, SqlitePool, Transaction};

use crate::db::{
    account::{BrowserCredential, LoginAccount},
    error::DatabaseError,
    repo::identity::bind as bind_identity,
};

const ACCOUNT_COLUMNS: &str = "id,username,password_hash,role,canonical_user_id,must_change_password,disabled,token_version,revision";

pub(crate) async fn by_id(
    pool: &SqlitePool,
    id: &str,
) -> Result<Option<LoginAccount>, sqlx::Error> {
    query_as(&format!(
        "SELECT {ACCOUNT_COLUMNS} FROM albert_login_accounts WHERE id=?"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
}
pub(crate) async fn by_name(
    pool: &SqlitePool,
    username: &str,
) -> Result<Option<LoginAccount>, sqlx::Error> {
    query_as(&format!(
        "SELECT {ACCOUNT_COLUMNS} FROM albert_login_accounts WHERE username=?"
    ))
    .bind(username)
    .fetch_optional(pool)
    .await
}
pub(crate) async fn list(pool: &SqlitePool) -> Result<Vec<LoginAccount>, sqlx::Error> {
    query_as(&format!(
        "SELECT {ACCOUNT_COLUMNS} FROM albert_login_accounts ORDER BY created_at,id LIMIT 1000"
    ))
    .fetch_all(pool)
    .await
}
pub(crate) async fn seed_owner(pool: &SqlitePool, hash: &str) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let seeded: bool = query_scalar("SELECT seeded FROM albert_auth_bootstrap WHERE id=1")
        .fetch_one(&mut *tx)
        .await?;
    if !seeded {
        query("INSERT INTO albert_login_accounts(id,username,password_hash,role) SELECT 'bootstrap-owner','owner',?,'owner' WHERE NOT EXISTS(SELECT 1 FROM albert_login_accounts)").bind(hash).execute(&mut *tx).await?;
        query("UPDATE albert_auth_bootstrap SET seeded=1 WHERE id=1")
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await
}
pub(crate) async fn insert_with_profile(
    pool: &SqlitePool,
    id: &str,
    username: &str,
    hash: &str,
    role: &str,
    user: &str,
) -> Result<(), DatabaseError> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    bind_identity(&mut tx, "web", id, user).await?;
    query("INSERT INTO albert_login_accounts(id,username,password_hash,role,canonical_user_id) VALUES (?,?,?,?,?)").bind(id).bind(username).bind(hash).bind(role).bind(user).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}
pub(crate) async fn credential(
    pool: &SqlitePool,
    id: &str,
) -> Result<Option<BrowserCredential>, sqlx::Error> {
    query_as("SELECT id,account_id,token_version,issued_at,expires_at,revoked_at FROM albert_browser_credentials WHERE id=?").bind(id).fetch_optional(pool).await
}
pub(crate) async fn issue(
    pool: &SqlitePool,
    id: &str,
    account: &LoginAccount,
    now: i64,
    expires: i64,
) -> Result<bool, sqlx::Error> {
    Ok(query("INSERT INTO albert_browser_credentials(id,account_id,token_version,issued_at,expires_at) SELECT ?,id,token_version,?,? FROM albert_login_accounts WHERE id=? AND revision=? AND disabled=0")
        .bind(id).bind(now).bind(expires).bind(&account.id).bind(account.revision).execute(pool).await?.rows_affected()==1)
}
pub(crate) async fn revoke(
    pool: &SqlitePool,
    id: &str,
    account_id: &str,
    now: i64,
) -> Result<(), sqlx::Error> {
    query("UPDATE albert_browser_credentials SET revoked_at=? WHERE id=? AND account_id=?")
        .bind(now)
        .bind(id)
        .bind(account_id)
        .execute(pool)
        .await?;
    Ok(())
}
pub(crate) async fn rotate(
    pool: &SqlitePool,
    account: &LoginAccount,
    username: &str,
    hash: &str,
) -> Result<bool, sqlx::Error> {
    Ok(query("UPDATE albert_login_accounts SET username=?,password_hash=?,must_change_password=0,token_version=token_version+1,revision=revision+1 WHERE id=? AND revision=? AND disabled=0")
        .bind(username).bind(hash).bind(&account.id).bind(account.revision).execute(pool).await?.rows_affected()==1)
}
pub(crate) async fn update_in(
    tx: &mut Transaction<'_, Sqlite>,
    account: &LoginAccount,
    role: &str,
    disabled: bool,
    revision: i64,
) -> Result<bool, sqlx::Error> {
    Ok(query("UPDATE albert_login_accounts SET role=?,disabled=?,token_version=token_version+1,revision=revision+1 WHERE id=? AND revision=?")
        .bind(role).bind(disabled).bind(&account.id).bind(revision).execute(&mut **tx).await?.rows_affected()==1)
}
pub(crate) async fn owners_in(tx: &mut Transaction<'_, Sqlite>) -> Result<i64, sqlx::Error> {
    query_scalar("SELECT COUNT(*) FROM albert_login_accounts WHERE role='owner' AND disabled=0")
        .fetch_one(&mut **tx)
        .await
}
/// Trusted profile/link service only, after proof checks. Never rebinds a TG identity.
pub(crate) async fn bind_profile_in(
    tx: &mut Transaction<'_, Sqlite>,
    account_id: &str,
    user: &str,
    expected: i64,
) -> Result<bool, sqlx::Error> {
    Ok(query("UPDATE albert_login_accounts SET canonical_user_id=?,revision=revision+1 WHERE id=? AND revision=? AND (canonical_user_id IS NULL OR canonical_user_id=?)")
        .bind(user).bind(account_id).bind(expected).bind(user).execute(&mut **tx).await?.rows_affected()==1)
}

/// Keep existing operator/store principals stable when a proven canonical
/// profile is used through a new browser transport.
pub(crate) async fn native_identity_principal(
    pool: &SqlitePool,
    user: &str,
) -> Result<String, sqlx::Error> {
    let identity:Option<(String,String)>=query_as("SELECT connector_id,external_actor_id FROM albert_identity_bindings WHERE user_id=? AND connector_id<>'web' ORDER BY CASE WHEN connector_id='telegram' THEN 0 WHEN connector_id='console' THEN 2 ELSE 1 END,connector_id,external_actor_id LIMIT 1").bind(user).fetch_optional(pool).await?;
    Ok(match identity {
        Some((connector, actor)) if connector == "console" => format!("console:{actor}"),
        Some((connector, actor)) => format!("{connector}:user:{actor}"),
        None => format!("web:user:{user}"),
    })
}
