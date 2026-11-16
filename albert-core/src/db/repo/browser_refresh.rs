use sqlx::{query, query_as, SqlitePool};

use super::account::ACCOUNT_COLUMNS;
use crate::db::account::LoginAccount;

pub struct NewRefresh<'a> {
    pub digest: &'a str,
    pub csrf: &'a str,
    pub expires_at: i64,
}
pub struct RefreshedAccess {
    pub account: LoginAccount,
    pub credential_id: String,
    pub csrf: String,
    pub expires_at: i64,
}

pub async fn issue(
    pool: &SqlitePool,
    id: &str,
    account: &LoginAccount,
    now: i64,
    expires: i64,
    refresh: NewRefresh<'_>,
) -> Result<bool, sqlx::Error> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let written = query("INSERT INTO albert_browser_credentials(id,account_id,token_version,issued_at,expires_at) SELECT ?,id,token_version,?,? FROM albert_login_accounts WHERE id=? AND revision=? AND disabled=0")
        .bind(id).bind(now).bind(expires).bind(&account.id).bind(account.revision)
        .execute(&mut *tx).await?.rows_affected();
    if written != 1 {
        return Ok(false);
    }
    query("INSERT INTO albert_browser_refresh(credential_id,token_digest,csrf,created_at,expires_at) VALUES (?,?,?,?,?)")
        .bind(id).bind(refresh.digest).bind(refresh.csrf).bind(now).bind(refresh.expires_at)
        .execute(&mut *tx).await?;
    // Expired refresh records cannot authorize new access, including after restart.
    query("DELETE FROM albert_browser_refresh WHERE expires_at<=?")
        .bind(now)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(true)
}

pub async fn renew(
    pool: &SqlitePool,
    digest: &str,
    now: i64,
    access_ttl: i64,
) -> Result<Option<RefreshedAccess>, sqlx::Error> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let row: Option<(String, String, String, i64)> = query_as("SELECT c.id,c.account_id,r.csrf,r.expires_at FROM albert_browser_refresh r JOIN albert_browser_credentials c ON c.id=r.credential_id JOIN albert_login_accounts a ON a.id=c.account_id WHERE r.token_digest=? AND r.expires_at>? AND c.revoked_at IS NULL AND a.disabled=0 AND c.token_version=a.token_version")
        .bind(digest).bind(now).fetch_optional(&mut *tx).await?;
    let Some((credential_id, account_id, csrf, refresh_expires)) = row else {
        return Ok(None);
    };
    let account: LoginAccount = query_as(&format!(
        "SELECT {ACCOUNT_COLUMNS} FROM albert_login_accounts WHERE id=?"
    ))
    .bind(account_id)
    .fetch_one(&mut *tx)
    .await?;
    let expires_at = (now + access_ttl).min(refresh_expires);
    query("UPDATE albert_browser_credentials SET expires_at=MAX(expires_at,?) WHERE id=?")
        .bind(expires_at)
        .bind(&credential_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Some(RefreshedAccess {
        account,
        credential_id,
        csrf,
        expires_at,
    }))
}
