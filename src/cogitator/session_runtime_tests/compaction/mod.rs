use std::{sync::Arc, time::Duration};

use tokio::time::{sleep, timeout};

use super::{message, reply, Fixture};
use crate::{
    context::{Settings, Tokenizer},
    history::{HistoryStore, Turn},
};

fn enable(fixture: &mut Fixture) {
    Arc::get_mut(&mut fixture.host).unwrap().config.context = Settings {
        enabled: true,
        tokenizer: Tokenizer::Bytes,
        window_tokens: 400_000,
        reserve_tokens: 320_000,
        response_tokens: 1000,
        compact_output_tokens: 1000,
        new_messages_percent: 10,
        ..Settings::default()
    };
}

#[tokio::test]
async fn automatic_session_compact_excludes_current_input_and_keeps_it_once() {
    let mut fixture = Fixture::new().await;
    enable(&mut fixture);
    let session = fixture.context("telegram", "7", "7").await;
    fixture
        .router
        .append(
            &session.root_history_key(),
            &[Turn::user("old facts ".repeat(2000))],
        )
        .await
        .unwrap();
    let mut replies = fixture.replies().await;
    fixture
        .host
        .clone()
        .handle(
            message("telegram", "7", "7", "UNIQUE_CURRENT_QUESTION", false, true),
            &fixture.ctx,
        )
        .await;
    assert_eq!(
        reply(&mut replies).await.payload_as::<String>().unwrap(),
        "answer from session-delayed"
    );
    let window = fixture
        .router
        .context(&session.root_history_key())
        .await
        .unwrap();
    assert!(window.compact.is_some());
    assert_eq!(window.messages.len(), 2);
    let requests = fixture.server.requests.lock().unwrap();
    let mut compact_calls = 0;
    for request in requests.iter() {
        if request["tools"]
            .as_array()
            .is_none_or(|tools| tools.is_empty())
        {
            compact_calls += 1;
            assert!(!request["messages"]
                .to_string()
                .contains("UNIQUE_CURRENT_QUESTION"));
        }
    }
    assert!(compact_calls > 0);
    assert_eq!(
        requests.last().unwrap()["messages"]
            .to_string()
            .matches("UNIQUE_CURRENT_QUESTION")
            .count(),
        1
    );
    drop(requests);
    fixture.host.stop_turns(&fixture.ctx).await;
}

#[tokio::test]
async fn cancel_interrupts_automatic_session_compaction_before_it_can_resume() {
    let mut fixture = Fixture::with_model("never-compact").await;
    enable(&mut fixture);
    let session = fixture.context("telegram", "7", "7").await;
    fixture
        .router
        .append(
            &session.root_history_key(),
            &[Turn::user("old facts ".repeat(2000))],
        )
        .await
        .unwrap();
    let mut replies = fixture.replies().await;
    fixture
        .host
        .clone()
        .handle(
            message("telegram", "7", "7", "current question", false, true),
            &fixture.ctx,
        )
        .await;
    fixture.request_count(1).await;
    timeout(
        Duration::from_secs(1),
        fixture.host.clone().handle(
            message("telegram", "7", "7", "/cancel", false, true),
            &fixture.ctx,
        ),
    )
    .await
    .unwrap();
    assert_eq!(
        reply(&mut replies).await.payload_as::<String>().unwrap(),
        "Stopped."
    );
    sleep(Duration::from_millis(300)).await;
    assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
    assert!(fixture
        .router
        .context(&session.root_history_key())
        .await
        .unwrap()
        .compact
        .is_none());
    fixture.host.stop_turns(&fixture.ctx).await;
}
