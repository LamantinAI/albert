use std::{sync::Arc, time::Duration};

use octo_connector_telegram::{IngressDecision, NativeFrame, TelegramHostPolicy, TurnFeedback};
use octo_core::{ChannelId, ConnectorId, Envelope, EventBus, EventKind, Filter, SubscribeOptions};
use octo_history::{SessionStore, SessionUpdate};
use tokio::time::{sleep, timeout};

use super::{message, Fixture};
use crate::{
    config::AuthMode,
    history::Turn,
    models::ModelPool,
    sessions::{telegram::SessionTelegramPolicy, SessionContext},
};

async fn enabled(model: &str) -> (Fixture, SessionContext) {
    let mut fixture = Fixture::with_model(model).await;
    Arc::get_mut(&mut fixture.host)
        .unwrap()
        .config
        .sessions
        .titles
        .enabled = true;
    let initial = fixture.context("telegram", "7", "7").await;
    let context = fixture.service.create_private(&initial, "").await.unwrap();
    (fixture, context)
}
async fn named(fixture: &Fixture, context: &SessionContext) -> String {
    timeout(Duration::from_secs(4), async {
        loop {
            let title = fixture
                .service
                .get(context, context.session_id())
                .await
                .unwrap()
                .title;
            if !title.is_empty() {
                return title;
            }
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap()
}
#[tokio::test]
async fn title_is_toolfree_minimal_nonblocking_and_not_requested_again() {
    let (fixture, context) = enabled("title-delayed").await;
    let mut replies = fixture.replies().await;
    fixture
        .host
        .clone()
        .handle(
            message(
                "telegram",
                "7",
                "7",
                "How do SQLite journals recover?",
                false,
                true,
            ),
            &fixture.ctx,
        )
        .await;
    timeout(Duration::from_secs(1), replies.next())
        .await
        .unwrap()
        .unwrap();
    fixture.request_count(2).await;
    // Metadata is still pending, while the canonical result and user reply exist.
    assert!(fixture
        .service
        .get(&context, context.session_id())
        .await
        .unwrap()
        .title
        .is_empty());
    assert_eq!(named(&fixture, &context).await, "SQLite journal recovery");
    {
        let requests = fixture.server.requests.lock().unwrap();
        let metadata = &requests[1];
        assert!(metadata["tools"]
            .as_array()
            .is_none_or(|tools| tools.is_empty()));
        assert_eq!(metadata["messages"].as_array().unwrap().len(), 2);
        let wire = metadata["messages"].to_string();
        assert!(wire.contains("How do SQLite journals recover?"));
        assert!(!wire.contains("answer from"));
        assert!(!wire.contains("Message source;"));
        assert_eq!(metadata["max_tokens"], 1024);
    }
    fixture
        .host
        .clone()
        .handle(
            message("telegram", "7", "7", "One more detail", false, true),
            &fixture.ctx,
        )
        .await;
    timeout(Duration::from_secs(1), replies.next())
        .await
        .unwrap()
        .unwrap();
    sleep(Duration::from_millis(30)).await;
    assert_eq!(fixture.server.requests.lock().unwrap().len(), 3);
}
#[tokio::test]
async fn manual_rename_wins_against_pending_generation_and_provider_failure_uses_excerpt() {
    let (fixture, context) = enabled("title-delayed").await;
    let mut replies = fixture.replies().await;
    fixture
        .host
        .clone()
        .handle(
            message("telegram", "7", "7", "Explain SQLite recovery", false, true),
            &fixture.ctx,
        )
        .await;
    timeout(Duration::from_secs(1), replies.next())
        .await
        .unwrap()
        .unwrap();
    fixture.request_count(2).await;
    let current = fixture
        .service
        .get(&context, context.session_id())
        .await
        .unwrap();
    fixture
        .service
        .history()
        .update_session(
            context.session_id(),
            SessionUpdate {
                expected_revision: current.revision,
                title: Some("My own name".into()),
                archived: None,
            },
        )
        .await
        .unwrap();
    sleep(Duration::from_millis(400)).await;
    assert_eq!(named(&fixture, &context).await, "My own name");
    let (fallback, context) = enabled("title-failing").await;
    fallback
        .host
        .clone()
        .handle(
            message(
                "telegram",
                "7",
                "7",
                "Исследование журналов SQLite",
                false,
                true,
            ),
            &fallback.ctx,
        )
        .await;
    assert_eq!(
        named(&fallback, &context).await,
        "Исследование журналов SQLite"
    );
    assert_eq!(fallback.server.requests.lock().unwrap().len(), 2);
}
#[tokio::test]
async fn explicitly_named_session_has_no_utility_model_request() {
    let (fixture, blank) = enabled("title-delayed").await;
    let named_context = fixture
        .service
        .create_private(&blank, "User supplied name")
        .await
        .unwrap();
    let mut replies = fixture.replies().await;
    fixture
        .host
        .clone()
        .handle(
            message("telegram", "7", "7", "A useful conversation", false, true),
            &fixture.ctx,
        )
        .await;
    timeout(Duration::from_secs(1), replies.next())
        .await
        .unwrap()
        .unwrap();
    sleep(Duration::from_millis(30)).await;
    assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
    assert_eq!(named(&fixture, &named_context).await, "User supplied name");
}

#[tokio::test]
async fn subscription_auth_failure_uses_the_same_bounded_metadata_fallback() {
    let (mut fixture, context) = enabled("title-delayed").await;
    {
        let host = Arc::get_mut(&mut fixture.host).unwrap();
        let mut config = host.config.clone();
        config.auth = AuthMode::Subscription;
        config.models = None;
        host.models = ModelPool::new(&config);
    }
    fixture
        .host
        .history
        .append(
            &context.root_history_key(),
            &[
                Turn::user("Проверка журналов SQLite"),
                Turn::assistant("Recorded native result"),
            ],
        )
        .await
        .unwrap();
    fixture.host.schedule_title(
        &Envelope::new(
            ConnectorId::new("telegram"),
            EventKind::new("chat.message"),
            "native exchange".to_owned(),
        )
        .with_context(context.clone()),
    );
    assert_eq!(named(&fixture, &context).await, "Проверка журналов SQLite");
    assert!(fixture.server.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn rename_keeps_running_native_attempt_and_presentation_intact() {
    let mut fixture = Fixture::with_model("never-rename").await;
    Arc::get_mut(&mut fixture.host)
        .unwrap()
        .config
        .stream_status = true;
    let original = fixture.context("telegram", "7", "7").await;
    let mut events = fixture
        .ctx
        .bus()
        .subscribe(Filter::by_kind("chat.typing"), SubscribeOptions::default())
        .await
        .unwrap();
    fixture
        .host
        .clone()
        .handle(
            message("telegram", "7", "7", "continue work", false, true),
            &fixture.ctx,
        )
        .await;
    let typing = timeout(Duration::from_secs(1), events.next())
        .await
        .unwrap()
        .unwrap();
    let token = typing.context.get::<TurnFeedback>().unwrap().clone();
    fixture.request_count(1).await;
    let policy =
        SessionTelegramPolicy::new(fixture.service.clone(), fixture.host.authority.clone());
    let frame = NativeFrame {
        connector: ConnectorId::new("telegram"),
        bot_id: 99,
        update_id: 500,
        message_id: 500,
        date: 1_800_000_500,
        chat_id: 7,
        actor_id: Some(7),
        chat_type: "private".into(),
        command_text: Some("/rename Running SQLite study".into()),
        digest: "rename-current".into(),
        media: false,
    };
    let decision = policy
        .admit(
            frame,
            Envelope::new(
                ConnectorId::new("telegram"),
                EventKind::new("chat.message"),
                "/rename Running SQLite study".to_owned(),
            )
            .with_channel(ChannelId::new("7")),
        )
        .await
        .unwrap();
    assert!(matches!(decision, IngressDecision::Control { .. }));
    assert!(token.is_open());
    assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
    let renamed = fixture
        .service
        .get(&original, original.session_id())
        .await
        .unwrap();
    assert_eq!(renamed.title, "Running SQLite study");
    fixture.host.stop_turns(&fixture.ctx).await;
    assert!(!token.is_open());
}
