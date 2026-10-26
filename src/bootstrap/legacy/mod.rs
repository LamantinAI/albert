mod sources;

use std::{collections::HashSet, path::Path};

use tracing::info;

use self::sources::telegram_ids;
use crate::{
    error::Result,
    service::session::SessionService,
    sessions::{Admission, AdmissionScope, ExternalIdentity, ReplyRoute},
};

pub(super) async fn import_verified(
    service: &SessionService,
    manifest: &Path,
    telegram_enabled: bool,
) -> Result<()> {
    let telegram = telegram_ids(manifest, telegram_enabled)?;
    let inventory = service.history().journal_inventory().await?;
    let mut admissions = Vec::new();
    let mut origins = HashSet::new();
    for source in &inventory {
        let connector = if source.key == "stdin" {
            Some("console")
        } else {
            telegram.first().map(String::as_str)
        };
        if let Some(connector) = connector {
            if let Some(admission) = historical_admission(connector, &source.key)? {
                origins.insert((connector.to_owned(), source.key.clone()));
                admissions.push(admission);
            }
        }
    }
    // A child's recorded connector remains authoritative even when another bot
    // is custodian of the formerly shared bare root. Give it its own empty root.
    for origin in service.legacy_child_origins().await? {
        if ((origin.connector == "console" && origin.channel == "stdin")
            || telegram.contains(&origin.connector))
            && origins.insert((origin.connector.clone(), origin.channel.clone()))
        {
            if let Some(admission) = historical_admission(&origin.connector, &origin.channel)? {
                admissions.push(admission);
            }
        }
    }
    service.migrate_legacy_defaults(&admissions).await?;
    info!(
        channels = admissions.len(),
        "automatic standard-session history migration complete"
    );
    Ok(())
}

fn historical_admission(connector: &str, channel: &str) -> Result<Option<Admission>> {
    if connector == "console" && channel == "stdin" {
        return Ok(Some(Admission::new(
            ExternalIdentity::new("console", "local")?,
            ReplyRoute::new(connector, channel, None)?,
            AdmissionScope::Private,
            "host",
        )?));
    }
    let Ok(id) = channel.parse::<i64>() else {
        return Ok(None);
    };
    let route = ReplyRoute::new(connector, channel, None)?;
    Ok(match id {
        1.. => Some(Admission::new(
            ExternalIdentity::new(connector, channel)?,
            route,
            AdmissionScope::Private,
            "host",
        )?),
        ..=-1 => Some(Admission::group(None, route, "host")?),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use std::{
        fs::{create_dir_all, write},
        path::Path,
        sync::Arc,
    };

    use octo_history::{
        AppendRequest, HistoryStore, PageRequest, SessionAccess, SessionJournal, SessionOrigin,
        SessionStore, SqliteHistory, Turn,
    };
    use serde_json::{from_value, json};
    use tempfile::tempdir;

    use super::{historical_admission, import_verified};
    use crate::{
        service::session::SessionService,
        sessions::{
            settings::SessionSettings, Admission, AdmissionScope, ExternalIdentity, ReplyRoute,
        },
        subagents::Task,
    };

    fn manifests(dir: &Path, ids: &[&str]) {
        create_dir_all(dir.join("connectors")).unwrap();
        write(
            dir.join("octo.toml"),
            "[connectors]\ndir = \"connectors\"\n",
        )
        .unwrap();
        for id in ids {
            write(
                dir.join(format!("connectors/{id}.toml")),
                format!("[connector]\nid = \"{id}\"\ntype = \"telegram\"\nowner_chat = 42\n"),
            )
            .unwrap();
        }
    }
    fn assignment() -> Task {
        from_value(json!({"task":"inspect historical context","models":["retired-model"],"tools":["historical-tool"]})).unwrap()
    }

    #[test]
    fn unsupported_storage_keys_are_left_for_verified_ingress_not_treated_as_routes() {
        for key in [
            "0".to_string(),
            "unknown-channel".into(),
            format!("session/{}", "x".repeat(4096)),
        ] {
            assert!(historical_admission("telegram", &key).unwrap().is_none());
        }
    }
    fn private(connector: &str, channel: &str) -> Admission {
        Admission::new(
            ExternalIdentity::new(connector, channel).unwrap(),
            ReplyRoute::new(connector, channel, None).unwrap(),
            AdmissionScope::Private,
            "host",
        )
        .unwrap()
    }
    fn append(key: &str, turns: Vec<Turn>) -> AppendRequest {
        AppendRequest {
            namespace: "old-host".into(),
            idempotency_key: key.into(),
            digest: key.into(),
            turns,
        }
    }

    #[tokio::test]
    async fn automatic_import_preserves_root_child_compact_and_receipts_and_repeats() {
        let dir = tempdir().unwrap();
        manifests(dir.path(), &["telegram"]);
        let history = Arc::new(
            SqliteHistory::open_retained(dir.path().join("history.db"))
                .await
                .unwrap(),
        );
        let accepted = history
            .append_journal(
                "42",
                &append(
                    "root",
                    vec![
                        Turn::user("old private input"),
                        Turn::assistant("old reply"),
                    ],
                ),
            )
            .await
            .unwrap();
        assert!(history
            .save_compact("42", None, accepted.record_ids[0], "old compact")
            .await
            .unwrap());
        let run = "albert/child-1-opaque";
        let child_key = format!("subagent/{run}");
        let child = history.append_journal(&child_key, &append("child", vec![Turn::user(json!({"run_id":run,"parent_run_id":"albert/root-1","origin_connector":"telegram","origin_channel":"42","assignment":assignment()}).to_string()), Turn::assistant("child result")])).await.unwrap();
        history
            .append("77", &[Turn::user("unverified private source")])
            .await
            .unwrap();
        history.append("subagent/forged", &[Turn::assistant("not a host header"), Turn::user(json!({"run_id":"forged","origin_connector":"telegram","origin_channel":"42","assignment":"claimed"}).to_string())]).await.unwrap();
        let service = SessionService::from_history(history.clone(), SessionSettings::default())
            .await
            .unwrap();
        import_verified(&service, &dir.path().join("octo.toml"), true)
            .await
            .unwrap();
        let context = service.admit(private("telegram", "42")).await.unwrap();
        let root = history
            .session_page(&context.journal(), PageRequest::default())
            .await
            .unwrap();
        assert_eq!(
            root.records
                .iter()
                .map(|record| record.id)
                .collect::<Vec<_>>(),
            accepted.record_ids
        );
        let compact = history
            .session_context(&context.journal())
            .await
            .unwrap()
            .compact
            .unwrap();
        assert_eq!(compact.content, "old compact");
        assert_eq!(compact.through_id, accepted.record_ids[0]);
        let found = history
            .find_request("old-host", "root")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.journal, context.journal());
        assert_eq!(found.receipt.record_ids, accepted.record_ids);
        let child_journal = SessionJournal::Child {
            session_id: context.session_id().clone(),
            run_id: run.into(),
        };
        assert_eq!(
            history
                .session_page(&child_journal, PageRequest::default())
                .await
                .unwrap()
                .records
                .iter()
                .map(|record| record.id)
                .collect::<Vec<_>>(),
            child.record_ids
        );
        import_verified(&service, &dir.path().join("octo.toml"), true)
            .await
            .unwrap();
        assert_eq!(
            service
                .admit(private("telegram", "42"))
                .await
                .unwrap()
                .session_id(),
            context.session_id()
        );
        let other = service.admit(private("telegram", "77")).await.unwrap();
        assert_eq!(
            history.load_session(&other.journal()).await.unwrap()[0].content,
            "unverified private source"
        );
        assert_eq!(
            history
                .session_page(&other.journal(), PageRequest::default())
                .await
                .unwrap()
                .records
                .len(),
            1
        );
        assert_eq!(
            history
                .journal_page("subagent/forged", PageRequest::default())
                .await
                .unwrap()
                .records
                .len(),
            2
        );
    }

    #[tokio::test]
    async fn deterministic_custodian_imports_all_numeric_channels_without_copying() {
        let dir = tempdir().unwrap();
        manifests(dir.path(), &["zeta", "telegram", "alpha"]);
        let history = Arc::new(
            SqliteHistory::open_retained(dir.path().join("history.db"))
                .await
                .unwrap(),
        );
        for key in ["42", "77", "-42", "stdin"] {
            history
                .append(key, &[Turn::user(format!("old {key}"))])
                .await
                .unwrap();
        }
        let child_run = "albert/child-2-opaque";
        history.append(&format!("subagent/{child_run}"), &[Turn::user(json!({"run_id":child_run,"parent_run_id":"albert/root","origin_connector":"alpha","origin_channel":"-42","assignment":assignment()}).to_string())]).await.unwrap();
        let service = SessionService::from_history(history.clone(), SessionSettings::default())
            .await
            .unwrap();
        // Migration depends on configured origins, not a token or live connector.
        import_verified(&service, &dir.path().join("octo.toml"), false)
            .await
            .unwrap();
        for key in ["42", "77", "-42"] {
            let session = history
                .get_default_session(&SessionOrigin {
                    connector: "telegram".into(),
                    channel: key.into(),
                })
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                history
                    .load_session(&SessionJournal::Root(session.id.clone()))
                    .await
                    .unwrap()[0]
                    .content,
                format!("old {key}")
            );
            if key.starts_with('-') {
                assert!(matches!(session.access, SessionAccess::GroupBound { .. }));
                let reminder = service
                    .context_for_reminder(
                        &session.id,
                        ReplyRoute::new("telegram", key, None).unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(reminder.session_id(), &session.id);
            }
        }
        let console = service
            .admit(
                Admission::new(
                    ExternalIdentity::new("console", "local").unwrap(),
                    ReplyRoute::new("console", "stdin", None).unwrap(),
                    AdmissionScope::Private,
                    "host",
                )
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            history.load_session(&console.journal()).await.unwrap()[0].content,
            "old stdin"
        );
        let other = history
            .get_default_session(&SessionOrigin {
                connector: "alpha".into(),
                channel: "-42".into(),
            })
            .await
            .unwrap()
            .unwrap();
        assert!(history
            .load_session(&SessionJournal::Root(other.id.clone()))
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            history
                .load_session(&SessionJournal::Child {
                    session_id: other.id,
                    run_id: child_run.into()
                })
                .await
                .unwrap()
                .len(),
            1
        );
        let separate = service.admit(private("zeta", "42")).await.unwrap();
        assert!(history
            .load_session(&separate.journal())
            .await
            .unwrap()
            .is_empty());
        import_verified(&service, &dir.path().join("octo.toml"), true)
            .await
            .unwrap();
        assert_eq!(
            history
                .load_session(&console.journal())
                .await
                .unwrap()
                .len(),
            1
        );
        assert!(history
            .journal_inventory()
            .await
            .unwrap()
            .iter()
            .all(|entry| !["42", "77", "-42", "stdin"].contains(&entry.key.as_str())));
    }

    #[tokio::test]
    async fn lexicographic_custodian_is_stable_without_literal_telegram_id() {
        let dir = tempdir().unwrap();
        manifests(dir.path(), &["zeta", "alpha"]);
        let history = Arc::new(
            SqliteHistory::open_retained(dir.path().join("history.db"))
                .await
                .unwrap(),
        );
        history
            .append("77", &[Turn::user("non-owner private history")])
            .await
            .unwrap();
        let service = SessionService::from_history(history.clone(), SessionSettings::default())
            .await
            .unwrap();
        import_verified(&service, &dir.path().join("octo.toml"), true)
            .await
            .unwrap();
        assert!(history
            .get_default_session(&SessionOrigin {
                connector: "alpha".into(),
                channel: "77".into()
            })
            .await
            .unwrap()
            .is_some());
        assert!(history
            .get_default_session(&SessionOrigin {
                connector: "zeta".into(),
                channel: "77".into()
            })
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn console_without_manifest_migrates_stdin_without_assuming_telegram() {
        let dir = tempdir().unwrap();
        let history = Arc::new(
            SqliteHistory::open_retained(dir.path().join("history.db"))
                .await
                .unwrap(),
        );
        history
            .append("stdin", &[Turn::user("old console input")])
            .await
            .unwrap();
        history
            .append("77", &[Turn::user("unconfigured transport")])
            .await
            .unwrap();
        let service = SessionService::from_history(history.clone(), SessionSettings::default())
            .await
            .unwrap();
        import_verified(&service, &dir.path().join("missing-octo.toml"), false)
            .await
            .unwrap();
        let origin = SessionOrigin {
            connector: "console".into(),
            channel: "stdin".into(),
        };
        let session = history.get_default_session(&origin).await.unwrap().unwrap();
        assert_eq!(
            history
                .load_session(&SessionJournal::Root(session.id))
                .await
                .unwrap()[0]
                .content,
            "old console input"
        );
        assert_eq!(
            history
                .journal_page("77", PageRequest::default())
                .await
                .unwrap()
                .records
                .len(),
            1
        );
        assert!(
            import_verified(&service, &dir.path().join("missing-octo.toml"), true)
                .await
                .is_err()
        );
    }
}
