//! Canonical migration union and exact legacy admin-ledger reconciliation.
use sha2::{Digest, Sha256};
use sqlx::{query, query_as, query_scalar, raw_sql, SqlitePool};

use crate::db::error::{DatabaseError, DatabaseResult};

pub(super) const SCHEMA_VERSION: i64 = 7;
const MIGRATIONS: &[(i64, &str)] = &[
    (
        1,
        include_str!("../../../migrations/0001_session_control.sql"),
    ),
    (
        2,
        include_str!("../../../migrations/0002_native_sessions.sql"),
    ),
    (
        3,
        include_str!("../../../migrations/0003_session_messaging.sql"),
    ),
    (
        4,
        include_str!("../../../migrations/0004_message_failure_notices.sql"),
    ),
    (
        5,
        include_str!("../../../migrations/0005_message_delivery_evidence.sql"),
    ),
    (6, include_str!("../../../migrations/0006_web_accounts.sql")),
    (
        7,
        include_str!("../../../migrations/0007_control_applications.sql"),
    ),
];
fn checksum(sql: &str) -> String {
    format!("{:x}", Sha256::digest(sql.as_bytes()))
}
fn admin_prefix(rows: &[(i64, String)]) -> bool {
    (3..=4).contains(&rows.len())
        && rows.iter().enumerate().all(|(index, (version, stored))| {
            let canonical = if index < 2 { index } else { index + 3 };
            *version == index as i64 + 1 && *stored == checksum(MIGRATIONS[canonical].1)
        })
}
fn validate(rows: &[(i64, String)]) -> DatabaseResult<bool> {
    if let Some((version, _)) = rows.last().filter(|(version, _)| *version > SCHEMA_VERSION) {
        return Err(DatabaseError::SchemaVersion(*version));
    }
    if rows
        .iter()
        .enumerate()
        .any(|(index, (version, _))| *version != index as i64 + 1)
    {
        return Err(DatabaseError::MigrationLedger);
    }
    if admin_prefix(rows) {
        return Ok(true);
    }
    for (index, (version, stored)) in rows.iter().enumerate() {
        if MIGRATIONS
            .get(index)
            .is_none_or(|(_, sql)| *stored != checksum(sql))
        {
            return Err(DatabaseError::MigrationChecksum(*version));
        }
    }
    Ok(false)
}
pub(super) async fn migrate(pool: &SqlitePool) -> DatabaseResult<()> {
    // Never claim octo-history's registry. Reconciliation and all schema/data
    // upgrades share ONE transaction; unknown/mixed ledgers are never rewritten.
    let mut tx = pool.begin().await?;
    query("CREATE TABLE IF NOT EXISTS albert_schema_migrations (version INTEGER PRIMARY KEY, checksum TEXT NOT NULL)").execute(&mut *tx).await?;
    let rows: Vec<(i64, String)> =
        query_as("SELECT version,checksum FROM albert_schema_migrations ORDER BY version")
            .fetch_all(&mut *tx)
            .await?;
    if validate(&rows)? {
        query("UPDATE albert_schema_migrations SET version=6 WHERE version=3")
            .execute(&mut *tx)
            .await?;
        if rows.len() == 4 {
            query("UPDATE albert_schema_migrations SET version=7 WHERE version=4")
                .execute(&mut *tx)
                .await?;
        }
    }
    for &(version, sql) in MIGRATIONS {
        let digest = checksum(sql);
        let previous: Option<String> =
            query_scalar("SELECT checksum FROM albert_schema_migrations WHERE version=?")
                .bind(version)
                .fetch_optional(&mut *tx)
                .await?;
        match previous {
            Some(previous) if previous != digest => {
                return Err(DatabaseError::MigrationChecksum(version))
            }
            Some(_) => {}
            None => {
                raw_sql(sql).execute(&mut *tx).await?;
                query("INSERT INTO albert_schema_migrations(version,checksum) VALUES (?,?)")
                    .bind(version)
                    .bind(digest)
                    .execute(&mut *tx)
                    .await?;
            }
        }
    }
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use octo_history::{
        AppendRequest, CreateSession, PageRequest, SessionId, SessionJournal, SessionStore,
        SqliteHistory, Turn,
    };
    use sqlx::{query, query_as, query_scalar, raw_sql, SqlitePool};
    use tempfile::{tempdir, TempDir};

    use super::{checksum, MIGRATIONS};
    use crate::db::{error::DatabaseError, Database};

    struct Fixture {
        _dir: TempDir,
        history: SqliteHistory,
        pool: SqlitePool,
        session: SessionId,
    }
    async fn fixture() -> Fixture {
        let dir = tempdir().unwrap();
        let history = SqliteHistory::open_retained(dir.path().join("history.sqlite"))
            .await
            .unwrap();
        let session = history
            .create_session(CreateSession::private("fixture-owner"))
            .await
            .unwrap()
            .id;
        history
            .append_session(
                &SessionJournal::Root(session.clone()),
                &AppendRequest {
                    namespace: "fixture".into(),
                    idempotency_key: "initial".into(),
                    digest: "canonical-fixture".into(),
                    turns: vec![Turn::user("retained journal payload")],
                },
            )
            .await
            .unwrap();
        let pool = history.pool().clone();
        Fixture {
            _dir: dir,
            history,
            pool,
            session,
        }
    }
    async fn seed(pool: &SqlitePool, lineage: &[(i64, &str)]) {
        let mut tx = pool.begin().await.unwrap();
        query("CREATE TABLE albert_schema_migrations(version INTEGER PRIMARY KEY,checksum TEXT NOT NULL)").execute(&mut *tx).await.unwrap();
        for &(version, sql) in lineage {
            raw_sql(sql).execute(&mut *tx).await.unwrap();
            query("INSERT INTO albert_schema_migrations VALUES (?,?)")
                .bind(version)
                .bind(checksum(sql))
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        tx.commit().await.unwrap();
    }
    fn admin() -> Vec<(i64, &'static str)> {
        vec![
            MIGRATIONS[0],
            MIGRATIONS[1],
            (3, MIGRATIONS[5].1),
            (4, MIGRATIONS[6].1),
        ]
    }
    async fn ledger(pool: &SqlitePool) -> Vec<(i64, String)> {
        query_as("SELECT version,checksum FROM albert_schema_migrations ORDER BY version")
            .fetch_all(pool)
            .await
            .unwrap()
    }
    async fn count(pool: &SqlitePool, name: &str) -> i64 {
        query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?")
            .bind(name)
            .fetch_one(pool)
            .await
            .unwrap()
    }
    async fn assert_complete(fixture: &Fixture) {
        assert_eq!(
            ledger(&fixture.pool).await,
            MIGRATIONS
                .iter()
                .map(|(version, sql)| (*version, checksum(sql)))
                .collect::<Vec<_>>()
        );
        for table in [
            "albert_messages",
            "albert_message_delivery_evidence",
            "albert_login_accounts",
            "albert_control_applications",
        ] {
            assert_eq!(count(&fixture.pool, table).await, 1);
        }
        let journal = fixture
            .history
            .session_page(
                &SessionJournal::Root(fixture.session.clone()),
                PageRequest::default(),
            )
            .await
            .unwrap();
        assert_eq!(journal.records.len(), 1);
        assert_eq!(journal.records[0].turn.content, "retained journal payload");
        Database::from_pool(fixture.pool.clone()).await.unwrap();
        assert_eq!(ledger(&fixture.pool).await.len(), 7);
    }
    async fn account_rows(pool: &SqlitePool) {
        query("INSERT INTO albert_users(id) VALUES ('fixture-owner')")
            .execute(pool)
            .await
            .unwrap();
        query("INSERT INTO albert_login_accounts(id,username,password_hash,role,canonical_user_id) VALUES ('account','local-fixture','placeholder-hash','owner','fixture-owner')").execute(pool).await.unwrap();
        query("INSERT INTO albert_browser_credentials(id,account_id,token_version,issued_at,expires_at) VALUES ('credential','account',1,10,20)").execute(pool).await.unwrap();
    }
    async fn control_row(pool: &SqlitePool) {
        query("INSERT INTO albert_control_applications(id,account_id,client_request_id,request_hash,target,revision,state,message,runtime_epoch) VALUES ('application','account','request','hash','agent','revision','committed','Recorded','fixture')").execute(pool).await.unwrap();
    }
    #[tokio::test]
    async fn fresh_and_common_prefix_databases_receive_both_table_families_idempotently() {
        for prefix in 0..=2 {
            let f = fixture().await;
            if prefix > 0 {
                seed(&f.pool, &MIGRATIONS[..prefix]).await;
            }
            Database::from_pool(f.pool.clone()).await.unwrap();
            assert_complete(&f).await;
        }
    }
    #[tokio::test]
    async fn deployed_runtime_delivery_and_interrupted_work_survive_admin_upgrade() {
        let f = fixture().await;
        seed(&f.pool, &MIGRATIONS[..5]).await;
        query("INSERT INTO albert_messages(message_id,actor_id,source_session,source_record,envelope,digest,created_at,deadline_unix_ms) VALUES ('message','fixture-owner',?,1,'{}','digest',10,20)").bind(f.session.as_str()).execute(&f.pool).await.unwrap();
        query("INSERT INTO albert_message_inbox(message_id,recipient_session,recipient_record,work_state,failure_code,report_outcomes) VALUES ('message',?,1,'unknown','interrupted','[]')").bind(f.session.as_str()).execute(&f.pool).await.unwrap();
        query("INSERT INTO albert_message_outbox(message_id,state,attempts,attempt_token,transport_kind,submitted_at,observation_deadline) VALUES ('message','unknown',1,'attempt','telegram',10,20)").execute(&f.pool).await.unwrap();
        query("INSERT INTO albert_message_delivery_evidence(message_id,attempt_token,observation_hash,outcome,native_ids,observed_at) VALUES ('message','attempt','observation','sent','[17]',11)").execute(&f.pool).await.unwrap();
        Database::from_pool(f.pool.clone()).await.unwrap();
        assert_complete(&f).await;
        let work: (String, String) = query_as(
            "SELECT work_state,failure_code FROM albert_message_inbox WHERE message_id='message'",
        )
        .fetch_one(&f.pool)
        .await
        .unwrap();
        assert_eq!(work, ("unknown".into(), "interrupted".into()));
        let outbox:(String,String,String)=query_as("SELECT state,attempt_token,transport_kind FROM albert_message_outbox WHERE message_id='message'").fetch_one(&f.pool).await.unwrap();
        assert_eq!(
            outbox,
            ("unknown".into(), "attempt".into(), "telegram".into())
        );
        let ids: String = query_scalar(
            "SELECT native_ids FROM albert_message_delivery_evidence WHERE message_id='message'",
        )
        .fetch_one(&f.pool)
        .await
        .unwrap();
        assert_eq!(ids, "[17]");
    }
    #[tokio::test]
    async fn known_admin_prefixes_remap_only_versions_and_preserve_accounts_controls_and_checksums()
    {
        for prefix in [3, 4] {
            let f = fixture().await;
            seed(&f.pool, &admin()[..prefix]).await;
            account_rows(&f.pool).await;
            if prefix == 4 {
                control_row(&f.pool).await;
            }
            let before = ledger(&f.pool).await;
            Database::from_pool(f.pool.clone()).await.unwrap();
            assert_complete(&f).await;
            let after = ledger(&f.pool).await;
            assert_eq!(after[5].1, before[2].1);
            if prefix == 4 {
                assert_eq!(after[6].1, before[3].1);
                let state: String = query_scalar(
                    "SELECT state FROM albert_control_applications WHERE id='application'",
                )
                .fetch_one(&f.pool)
                .await
                .unwrap();
                assert_eq!(state, "committed");
            }
            let account:(String,String,String)=query_as("SELECT username,canonical_user_id,password_hash FROM albert_login_accounts WHERE id='account'").fetch_one(&f.pool).await.unwrap();
            assert_eq!(
                account,
                (
                    "local-fixture".into(),
                    "fixture-owner".into(),
                    "placeholder-hash".into()
                )
            );
            let credentials: i64 = query_scalar(
                "SELECT COUNT(*) FROM albert_browser_credentials WHERE account_id='account'",
            )
            .fetch_one(&f.pool)
            .await
            .unwrap();
            assert_eq!(credentials, 1);
        }
    }
    #[tokio::test]
    async fn late_failure_rolls_back_admin_remap_and_all_runtime_schema_changes() {
        let f = fixture().await;
        seed(&f.pool, &admin()).await;
        account_rows(&f.pool).await;
        control_row(&f.pool).await;
        query("CREATE TABLE albert_message_delivery_evidence(injected_failure TEXT)")
            .execute(&f.pool)
            .await
            .unwrap();
        let before = ledger(&f.pool).await;
        assert!(Database::from_pool(f.pool.clone()).await.is_err());
        assert_eq!(ledger(&f.pool).await, before);
        assert_eq!(count(&f.pool, "albert_messages").await, 0);
        let accounts: i64 = query_scalar("SELECT COUNT(*) FROM albert_login_accounts")
            .fetch_one(&f.pool)
            .await
            .unwrap();
        assert_eq!(accounts, 1);
        query("DROP TABLE albert_message_delivery_evidence")
            .execute(&f.pool)
            .await
            .unwrap();
        Database::from_pool(f.pool.clone()).await.unwrap();
        assert_complete(&f).await;
    }
    #[tokio::test]
    async fn unknown_tampered_and_mixed_ledgers_are_rejected_before_reconciliation() {
        for variant in 0..4 {
            let f = fixture().await;
            if variant < 2 {
                seed(&f.pool, &admin()).await;
            } else {
                seed(&f.pool, &MIGRATIONS[..2]).await;
            }
            match variant {
                0 => {
                    query(
                        "UPDATE albert_schema_migrations SET checksum='tampered' WHERE version=4",
                    )
                    .execute(&f.pool)
                    .await
                    .unwrap();
                }
                1 => {
                    query("INSERT INTO albert_schema_migrations VALUES (5,?)")
                        .bind(checksum(MIGRATIONS[4].1))
                        .execute(&f.pool)
                        .await
                        .unwrap();
                }
                2 => {
                    query("INSERT INTO albert_schema_migrations VALUES (6,?)")
                        .bind(checksum(MIGRATIONS[5].1))
                        .execute(&f.pool)
                        .await
                        .unwrap();
                }
                _ => {
                    query("INSERT INTO albert_schema_migrations VALUES (8,'unknown')")
                        .execute(&f.pool)
                        .await
                        .unwrap();
                }
            }
            let before = ledger(&f.pool).await;
            let error = Database::from_pool(f.pool.clone()).await.err().unwrap();
            assert!(matches!(
                error,
                DatabaseError::MigrationChecksum(_)
                    | DatabaseError::MigrationLedger
                    | DatabaseError::SchemaVersion(8)
            ));
            assert_eq!(ledger(&f.pool).await, before);
            assert_eq!(count(&f.pool, "albert_messages").await, 0);
        }
    }
}
