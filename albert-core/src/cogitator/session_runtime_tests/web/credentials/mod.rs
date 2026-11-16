use super::{actor, configure_web, envelope, next, Fixture};
use crate::{
    cogitator::{credentials::receipt_text, UserInput},
    credentials::{
        CredentialContinuation, CredentialGuard, CredentialNotification, CredentialState,
    },
    selfconfig::SelfConfig,
    sessions::{actor::ActorToolPolicy, SessionContext},
};
use sqlx::query;
use std::{sync::Arc, time::Duration};
use tokio::time::timeout;

async fn conversation(fixture: &mut Fixture) -> SessionContext {
    configure_web(fixture);
    fixture.service.credentials().enable_ui();
    let owner = actor(fixture, "account", ActorToolPolicy::ExistingHost).await;
    query("INSERT INTO albert_login_accounts(id,username,password_hash,role,canonical_user_id,must_change_password) VALUES ('account','fixture','unused','owner','web-fixture-user',0)").execute(fixture.service.database().pool()).await.unwrap();
    let session = fixture
        .service
        .web_create(&owner, "Credential fixture", false)
        .await
        .unwrap();
    fixture
        .service
        .web_context(&owner, &session.id)
        .await
        .unwrap()
        .with_account(fixture.service.accounts().account("account").await.unwrap())
}
#[tokio::test]
async fn credential_receipt_waits_for_idle_and_never_exposes_the_value_to_model_or_journal() {
    let mut fixture = Fixture::with_model("credential-handoff").await;
    let context = conversation(&mut fixture).await;
    let mut replies = fixture.replies().await;
    fixture
        .host
        .clone()
        .handle(
            Arc::new(envelope(
                &context,
                "credential-question",
                "Configure the service",
            )),
            &fixture.ctx,
        )
        .await;
    next(&mut replies, "chat.reply").await;
    let request = fixture
        .service
        .credentials()
        .list(
            "account",
            "web-fixture-user",
            Some(context.session_id().as_str()),
        )
        .remove(0);
    assert_eq!(request.suggested_name, "EXPECTED_TOKEN");
    let sentinel = "sensitive-fixture-value-49";
    let store = SelfConfig::new(fixture._directory.path().into());
    fixture
        .service
        .credentials()
        .save(
            "account",
            "web-fixture-user",
            &request.id,
            "ACTUAL_TOKEN",
            || {
                store
                    .panel_upsert_secret("ACTUAL_TOKEN", sentinel)
                    .map_err(|_| ())
            },
        )
        .unwrap();
    // Simulate an input append followed by a preparation failure before task scheduling.
    // The pending receipt must recover through the duplicate-input path exactly once.
    let text = receipt_text("ACTUAL_TOKEN");
    let preaccepted = envelope(&context, &request.id, &text).with_context(CredentialContinuation {
        id: request.id.clone(),
    });
    fixture
        .host
        .append_native(
            &preaccepted,
            &UserInput {
                text,
                images: vec![],
                seed: None,
                voice: None,
            },
        )
        .await
        .unwrap();
    fixture
        .host
        .clone()
        .handle(
            Arc::new(envelope(
                &context,
                "other-work",
                "Keep doing the other task",
            )),
            &fixture.ctx,
        )
        .await;
    fixture.request_count(3).await;
    let active = fixture
        .host
        .panel_active(context.session_id())
        .await
        .unwrap();
    fixture.host.panel_maintenance(&fixture.ctx).await;
    assert_eq!(
        fixture
            .host
            .panel_active(context.session_id())
            .await
            .as_deref(),
        Some(active.as_str())
    );
    assert_eq!(fixture.server.requests.lock().unwrap().len(), 3);
    next(&mut replies, "chat.reply").await;
    fixture.host.panel_maintenance(&fixture.ctx).await;
    next(&mut replies, "chat.reply").await;
    let requests = fixture.server.requests.lock().unwrap().clone();
    assert_eq!(requests.len(), 4);
    assert!(requests[3]["messages"].to_string().contains("ACTUAL_TOKEN"));
    assert!(!requests
        .iter()
        .any(|request| request.to_string().contains(sentinel)));
    let history = fixture
        .router
        .strict_load(&context.root_history_key())
        .await
        .unwrap();
    assert!(!history.iter().any(|turn| turn.content.contains(sentinel)));
    assert_eq!(
        history
            .iter()
            .filter(|turn| turn.content.contains("[Credential handoff receipt]"))
            .count(),
        1
    );
    assert_eq!(
        fixture
            .service
            .credentials()
            .get("account", "web-fixture-user", &request.id)
            .unwrap()
            .notification,
        CredentialNotification::Recorded
    );
    fixture.host.panel_maintenance(&fixture.ctx).await;
    assert_eq!(fixture.server.requests.lock().unwrap().len(), 4);
    let mut child_context = context.clone().without_tools();
    assert!(!CredentialGuard::agent_allowed(&child_context));
    child_context = context;
    assert!(CredentialGuard::agent_allowed(&child_context));
    fixture.host.stop_turns(&fixture.ctx).await;
}
#[tokio::test]
async fn interruption_before_the_proposal_completes_invalidates_the_form() {
    let mut fixture = Fixture::with_model("credential-cancel").await;
    let context = conversation(&mut fixture).await;
    fixture
        .host
        .clone()
        .handle(
            Arc::new(envelope(&context, "proposal", "Configure the service")),
            &fixture.ctx,
        )
        .await;
    fixture.request_count(2).await;
    let request = fixture
        .service
        .credentials()
        .list(
            "account",
            "web-fixture-user",
            Some(context.session_id().as_str()),
        )
        .remove(0);
    let attempt = fixture
        .host
        .panel_active(context.session_id())
        .await
        .unwrap();
    timeout(
        Duration::from_secs(2),
        fixture
            .host
            .panel_cancel_exact(context.session_id(), &attempt, &fixture.ctx),
    )
    .await
    .unwrap();
    assert_eq!(
        fixture
            .service
            .credentials()
            .get("account", "web-fixture-user", &request.id)
            .unwrap()
            .state,
        CredentialState::Cancelled
    );
}
