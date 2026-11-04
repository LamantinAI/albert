//! The first real agent turn after an upgrade must see the old channel journal,
//! without an operator editing SQL or configuring a source mapping.

use octo_history::{
    HistoryError, HistoryStore, PageRequest, SessionAccess, SessionOrigin, SessionStore, Turn,
};

use super::{message, reply, Fixture};

#[tokio::test]
async fn first_private_turn_keeps_legacy_context_and_responds_in_the_same_channel() {
    let fixture = Fixture::new().await;
    let mut replies = fixture.replies().await;
    fixture
        .service
        .history()
        .append(
            "legacy-private-channel",
            &[
                Turn::user("LEGACY_PRIVATE_QUESTION"),
                Turn::assistant("LEGACY_PRIVATE_ANSWER"),
            ],
        )
        .await
        .unwrap();
    fixture
        .host
        .clone()
        .handle(
            message(
                "telegram",
                "7",
                "legacy-private-channel",
                "Continue the earlier conversation",
                false,
                true,
            ),
            &fixture.ctx,
        )
        .await;
    let answer = reply(&mut replies).await;
    assert_eq!(answer.target.as_ref().unwrap().as_str(), "telegram");
    assert_eq!(
        answer.channel.as_ref().unwrap().as_str(),
        "legacy-private-channel"
    );
    let request = fixture.server.requests.lock().unwrap()[0]["messages"].to_string();
    assert!(request.contains("LEGACY_PRIVATE_QUESTION"));
    assert!(request.contains("LEGACY_PRIVATE_ANSWER"));
    let context = fixture
        .context("telegram", "7", "legacy-private-channel")
        .await;
    assert_eq!(
        fixture
            .router
            .strict_load(&context.root_history_key())
            .await
            .unwrap()
            .len(),
        4
    );
    assert!(matches!(fixture.service.history()
        .journal_page("legacy-private-channel", PageRequest::default()).await,
        Err(HistoryError::KeyRemapped { target_key, .. }) if target_key == context.root_history_key()));
    fixture.host.stop_turns(&fixture.ctx).await;
}

#[tokio::test]
async fn first_group_turn_uses_the_shared_old_journal_without_assigning_it_to_the_sender() {
    let fixture = Fixture::new().await;
    let mut replies = fixture.replies().await;
    fixture
        .service
        .history()
        .append(
            "legacy-group-channel",
            &[
                Turn::user("LEGACY_GROUP_QUESTION"),
                Turn::assistant("LEGACY_GROUP_ANSWER"),
            ],
        )
        .await
        .unwrap();
    fixture
        .host
        .clone()
        .handle(
            message(
                "telegram",
                "8",
                "legacy-group-channel",
                "Continue the group conversation",
                true,
                false,
            ),
            &fixture.ctx,
        )
        .await;
    let answer = reply(&mut replies).await;
    assert_eq!(answer.target.as_ref().unwrap().as_str(), "telegram");
    assert_eq!(
        answer.channel.as_ref().unwrap().as_str(),
        "legacy-group-channel"
    );
    let request = fixture.server.requests.lock().unwrap()[0]["messages"].to_string();
    assert!(request.contains("LEGACY_GROUP_QUESTION"));
    assert!(request.contains("LEGACY_GROUP_ANSWER"));
    let session = fixture
        .service
        .history()
        .get_default_session(&SessionOrigin {
            connector: "telegram".into(),
            channel: "legacy-group-channel".into(),
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        session.access,
        SessionAccess::GroupBound {
            connector: "telegram".into(),
            channel: "legacy-group-channel".into(),
        }
    );
    fixture.host.stop_turns(&fixture.ctx).await;
}

#[tokio::test]
async fn first_turn_after_migration_uses_the_existing_compact_and_its_remaining_messages() {
    let fixture = Fixture::new().await;
    let mut replies = fixture.replies().await;
    let history = fixture.service.history();
    history
        .append(
            "legacy-compacted-channel",
            &[
                Turn::user("OLD_INPUT_ALREADY_COMPACTED"),
                Turn::assistant("OLD_ANSWER_AFTER_COMPACT_BOUNDARY"),
            ],
        )
        .await
        .unwrap();
    let records = history
        .journal_page("legacy-compacted-channel", PageRequest::default())
        .await
        .unwrap();
    assert!(history
        .save_compact(
            "legacy-compacted-channel",
            None,
            records.records[0].id,
            "PRESERVED_LEGACY_COMPACT"
        )
        .await
        .unwrap());
    fixture
        .host
        .clone()
        .handle(
            message(
                "telegram",
                "7",
                "legacy-compacted-channel",
                "Continue with the compacted context",
                false,
                true,
            ),
            &fixture.ctx,
        )
        .await;
    reply(&mut replies).await;
    let wire = fixture.server.requests.lock().unwrap()[0]["messages"].to_string();
    assert!(wire.contains("PRESERVED_LEGACY_COMPACT"));
    assert!(wire.contains("OLD_ANSWER_AFTER_COMPACT_BOUNDARY"));
    assert!(!wire.contains("OLD_INPUT_ALREADY_COMPACTED"));
    let context = fixture
        .context("telegram", "7", "legacy-compacted-channel")
        .await;
    let compact = history
        .session_context(&context.journal())
        .await
        .unwrap()
        .compact
        .unwrap();
    assert_eq!(compact.content, "PRESERVED_LEGACY_COMPACT");
    assert_eq!(compact.through_id, records.records[0].id);
    fixture.host.stop_turns(&fixture.ctx).await;
}
