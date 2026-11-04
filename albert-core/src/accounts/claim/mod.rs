use std::collections::HashSet;

use octo_history::SqliteHistory;
use sqlx::{query, query_as, query_scalar, Sqlite, Transaction};

use super::{AccountError, AccountResult};

/// An internal, proof-gated legacy upgrade, not a general account merger.
/// Current account data and public group journals never change ownership.
pub(super) async fn reconcile(
    history: &SqliteHistory,
    tx: &mut Transaction<'_, Sqlite>,
    account: &str,
    target: &str,
    connector: &str,
    actor: &str,
) -> AccountResult<()> {
    let other_target_accounts: i64 = query_scalar(
        "SELECT COUNT(*) FROM albert_login_accounts WHERE canonical_user_id=? AND id<>?",
    )
    .bind(target)
    .bind(account)
    .fetch_one(&mut **tx)
    .await?;
    if other_target_accounts > 0 {
        return Err(AccountError::AlreadyBound);
    }
    let old: Option<String> = query_scalar(
        "SELECT user_id FROM albert_identity_bindings WHERE connector_id=? AND external_actor_id=?",
    )
    .bind(connector)
    .bind(actor)
    .fetch_optional(&mut **tx)
    .await?;
    query("INSERT INTO albert_users(id) VALUES (?) ON CONFLICT(id) DO NOTHING")
        .bind(target)
        .execute(&mut **tx)
        .await?;
    let Some(old) = old else {
        query("INSERT INTO albert_identity_bindings(connector_id,external_actor_id,user_id) VALUES (?,?,?)")
            .bind(connector).bind(actor).bind(target).execute(&mut **tx).await?;
        return Ok(());
    };
    if old == target {
        return Ok(());
    }
    let owners: i64 =
        query_scalar("SELECT COUNT(*) FROM albert_login_accounts WHERE canonical_user_id=?")
            .bind(&old)
            .fetch_one(&mut **tx)
            .await?;
    let web_owners: i64 = query_scalar(
        "SELECT COUNT(*) FROM albert_identity_bindings WHERE user_id=? AND connector_id='web'",
    )
    .bind(&old)
    .fetch_one(&mut **tx)
    .await?;
    let claim: Option<String> =
        query_scalar("SELECT account_id FROM albert_telegram_legacy_claims WHERE legacy_user_id=?")
            .bind(&old)
            .fetch_optional(&mut **tx)
            .await?;
    if owners > 0 || web_owners > 0 || claim.as_ref().is_some_and(|id| id != account) {
        return Err(AccountError::AlreadyBound);
    }
    let ids = history.private_session_ids_in(tx, &old).await?;
    history
        .reassign_private_owner_in(tx, &ids, &old, target)
        .await?;
    let ids: HashSet<String> = ids.into_iter().map(|id| id.to_string()).collect();
    let routes: Vec<(String, String, String, Option<String>, i64)> = query_as("SELECT connector_id,channel_id,session_id,return_session_id,revision FROM albert_private_session_routes WHERE user_id=?")
        .bind(&old).fetch_all(&mut **tx).await?;
    for (source, channel, session, returning, revision) in routes {
        if !ids.contains(&session) {
            continue;
        }
        query("INSERT INTO albert_private_session_routes(connector_id,channel_id,user_id,session_id,return_session_id,revision) VALUES (?,?,?,?,?,?) ON CONFLICT(connector_id,channel_id,user_id) DO UPDATE SET session_id=excluded.session_id,return_session_id=excluded.return_session_id,revision=albert_private_session_routes.revision+1")
            .bind(&source).bind(&channel).bind(target).bind(&session).bind(returning).bind(revision).execute(&mut **tx).await?;
        query("DELETE FROM albert_private_session_routes WHERE connector_id=? AND channel_id=? AND user_id=?")
            .bind(source).bind(channel).bind(&old).execute(&mut **tx).await?;
    }
    for id in &ids {
        query("UPDATE albert_session_markers SET owner_user_id=? WHERE session_id=? AND owner_user_id=?")
            .bind(target).bind(id).bind(&old).execute(&mut **tx).await?;
        query("UPDATE albert_temporary_excursions SET user_id=? WHERE session_id=? AND user_id=?")
            .bind(target)
            .bind(id)
            .bind(&old)
            .execute(&mut **tx)
            .await?;
    }
    query("UPDATE albert_identity_bindings SET user_id=? WHERE connector_id=? AND external_actor_id=? AND user_id=?")
        .bind(target).bind(connector).bind(actor).bind(&old).execute(&mut **tx).await?;
    query("INSERT INTO albert_telegram_legacy_claims(legacy_user_id,account_id,canonical_user_id) VALUES (?,?,?) ON CONFLICT(legacy_user_id) DO NOTHING")
        .bind(old).bind(account).bind(target).execute(&mut **tx).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::accounts::tests::fixture;
    use octo_history::{
        AppendRequest, CreateSession, PageRequest, SessionAccess, SessionJournal, SessionOrigin,
        SessionStore, Turn,
    };
    use sqlx::{query, query_scalar};

    #[tokio::test]
    async fn login_retains_selected_legacy_chat_and_existing_ui_history() {
        let (_dir, accounts) = fixture().await;
        let origin = SessionOrigin {
            connector: "telegram".into(),
            channel: "7".into(),
        };
        let original = accounts
            .history
            .ensure_default_session(&origin, CreateSession::private("legacy"))
            .await
            .unwrap();
        let saved = accounts
            .history
            .create_session(CreateSession::private("legacy"))
            .await
            .unwrap();
        let ui = accounts
            .history
            .create_session(CreateSession::private("profile"))
            .await
            .unwrap();
        let journal = SessionJournal::Root(saved.id.clone());
        let request = AppendRequest {
            namespace: "fixture".into(),
            idempotency_key: "original".into(),
            digest: "same-payload".into(),
            turns: vec![
                Turn::user("Existing question"),
                Turn::assistant("Existing answer"),
            ],
        };
        let root_receipt = accounts
            .history
            .append_session(&journal, &request)
            .await
            .unwrap();
        assert!(accounts
            .history
            .save_session_compact(&journal, None, root_receipt.through_id, "Existing compact")
            .await
            .unwrap());
        let child = SessionJournal::Child {
            session_id: saved.id.clone(),
            run_id: "original-child".into(),
        };
        let child_request = AppendRequest {
            namespace: "fixture-child".into(),
            idempotency_key: "child".into(),
            digest: "same-child".into(),
            turns: vec![Turn::assistant("Existing child result")],
        };
        let child_receipt = accounts
            .history
            .append_session(&child, &child_request)
            .await
            .unwrap();
        let compact_before = accounts
            .history
            .session_context(&journal)
            .await
            .unwrap()
            .compact
            .unwrap();
        let before = accounts
            .history
            .session_page(&journal, PageRequest::default())
            .await
            .unwrap();
        query(
            "INSERT INTO albert_private_session_routes VALUES ('telegram','7','legacy',?,NULL,4)",
        )
        .bind(saved.id.as_str())
        .execute(&accounts.pool)
        .await
        .unwrap();
        let mut group = CreateSession::private("unused");
        group.access = SessionAccess::GroupBound {
            connector: "telegram".into(),
            channel: "-10".into(),
        };
        let group = accounts.history.create_session(group).await.unwrap();
        let token = accounts.issue("account", "telegram").await.unwrap();
        accounts
            .login("telegram", 99, 7, 7, &token.token)
            .await
            .unwrap();
        let selected: String = query_scalar("SELECT session_id FROM albert_private_session_routes WHERE user_id='profile' AND connector_id='telegram' AND channel_id='7'").fetch_one(&accounts.pool).await.unwrap();
        assert_eq!(selected, saved.id.as_str());
        assert_eq!(
            accounts
                .history
                .get_default_session(&origin)
                .await
                .unwrap()
                .unwrap()
                .id,
            original.id
        );
        for id in [&original.id, &saved.id, &ui.id] {
            assert_eq!(
                accounts
                    .history
                    .get_session(id)
                    .await
                    .unwrap()
                    .unwrap()
                    .access,
                SessionAccess::Private {
                    owner: "profile".into()
                }
            );
        }
        assert_eq!(
            accounts
                .history
                .get_session(&group.id)
                .await
                .unwrap()
                .unwrap()
                .access,
            group.access
        );
        let after = accounts
            .history
            .session_page(&journal, PageRequest::default())
            .await
            .unwrap();
        assert_eq!(
            before.records.iter().map(|r| r.id).collect::<Vec<_>>(),
            after.records.iter().map(|r| r.id).collect::<Vec<_>>()
        );
        assert_eq!(after.records[1].turn.content, "Existing answer");
        let compact_after = accounts
            .history
            .session_context(&journal)
            .await
            .unwrap()
            .compact
            .unwrap();
        assert_eq!(compact_before.id, compact_after.id);
        assert_eq!(compact_after.content, "Existing compact");
        let child_after = accounts
            .history
            .session_page(&child, PageRequest::default())
            .await
            .unwrap();
        assert_eq!(child_after.records[0].id, child_receipt.record_ids[0]);
        assert_eq!(child_after.records[0].turn.content, "Existing child result");
        assert!(
            accounts
                .history
                .append_session(&child, &child_request)
                .await
                .unwrap()
                .duplicate
        );
        assert!(
            accounts
                .history
                .append_session(&journal, &request)
                .await
                .unwrap()
                .duplicate
        );
    }
}
