use std::{sync::Arc, time::Duration};

use sqlx::query;
use tokio::time::timeout;

use super::{actor, configure_web, envelope, next, Fixture};
use crate::{confirmations::ConfirmationState, sessions::actor::ActorToolPolicy};

#[tokio::test]
async fn pending_confirmation_yields_after_journaling_and_fences_siblings_then_resumes_once() {
    for approve in [true, false] {
        let mut fixture = Fixture::with_model(if approve {
            "confirmation-yes"
        } else {
            "confirmation-no"
        })
        .await;
        configure_web(&mut fixture);
        fixture.service.confirmations().enable_ui();
        let owner = actor(&fixture, "account", ActorToolPolicy::ExistingHost).await;
        query("INSERT INTO albert_login_accounts(id,username,password_hash,role,canonical_user_id,must_change_password) VALUES ('account','fixture','unused','owner','web-fixture-user',0)").execute(fixture.service.database().pool()).await.unwrap();
        let session = fixture
            .service
            .web_create(&owner, "Confirmation runtime", false)
            .await
            .unwrap();
        let context = fixture
            .service
            .web_context(&owner, &session.id)
            .await
            .unwrap()
            .with_account(fixture.service.accounts().account("account").await.unwrap());
        let mut replies = fixture.replies().await;
        fixture
            .host
            .clone()
            .handle(
                Arc::new(envelope(
                    &context,
                    "question",
                    "Review this plan before proceeding",
                )),
                &fixture.ctx,
            )
            .await;
        let answer = timeout(Duration::from_secs(3), async {
            loop {
                let event = replies.next().await.unwrap();
                if event.kind.as_str() == "chat.reply" {
                    break event;
                }
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!(
                "initial confirmation reply missing; requests={}, requests_pending={}",
                fixture.server.requests.lock().unwrap().len(),
                fixture
                    .service
                    .confirmations()
                    .list("account", "web-fixture-user", None)
                    .len()
            )
        });
        assert!(answer
            .payload_as::<String>()
            .unwrap()
            .contains("Waiting for your confirmation"));
        assert_eq!(
            fixture.server.requests.lock().unwrap().len(),
            1,
            "no retry, fallback or extra completion"
        );
        assert!(!fixture
            .host
            .config
            .code_workspace
            .join("should-not-run.txt")
            .exists());
        let request = fixture
            .service
            .confirmations()
            .list(
                "account",
                "web-fixture-user",
                Some(context.session_id().as_str()),
            )
            .remove(0);
        assert_eq!(request.state, ConfirmationState::Pending);
        let journal = fixture
            .host
            .strict_history(&context.root_history_key())
            .await
            .unwrap();
        let wire = serde_json::to_string(&journal).unwrap();
        assert!(wire.contains("request_confirmation"));
        assert!(wire.contains("NOT executed"));
        assert!(!wire.contains("outcome is UNKNOWN"));
        assert!(fixture
            .host
            .panel_active(context.session_id())
            .await
            .is_none());
        fixture
            .service
            .confirmations()
            .decide("account", "web-fixture-user", &request.id, approve)
            .await
            .unwrap();
        fixture.host.panel_maintenance(&fixture.ctx).await;
        fixture.host.panel_maintenance(&fixture.ctx).await;
        timeout(Duration::from_secs(3), next(&mut replies, "chat.reply"))
            .await
            .expect("decision continuation reply missing");
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 2);
        let requests = fixture.server.requests.lock().unwrap();
        let last = requests.last().unwrap().to_string();
        assert!(last.contains(if approve {
            "owner confirmed"
        } else {
            "owner denied"
        }));
        drop(requests);
        fixture.host.panel_maintenance(&fixture.ctx).await;
        assert!(timeout(Duration::from_millis(40), replies.next())
            .await
            .is_err());
    }
}

#[tokio::test]
async fn deleting_a_chat_cancels_its_waiting_question_and_never_schedules_a_continuation() {
    let mut fixture = Fixture::with_model("confirmation-delete").await;
    configure_web(&mut fixture);
    fixture.service.confirmations().enable_ui();
    let owner = actor(&fixture, "account", ActorToolPolicy::ExistingHost).await;
    query("INSERT INTO albert_login_accounts(id,username,password_hash,role,canonical_user_id,must_change_password) VALUES ('account','fixture','unused','owner','web-fixture-user',0)").execute(fixture.service.database().pool()).await.unwrap();
    let session = fixture
        .service
        .web_create(&owner, "Deleted while waiting", false)
        .await
        .unwrap();
    let context = fixture
        .service
        .web_context(&owner, &session.id)
        .await
        .unwrap()
        .with_account(fixture.service.accounts().account("account").await.unwrap());
    let mut replies = fixture.replies().await;
    fixture
        .host
        .clone()
        .handle(
            Arc::new(envelope(&context, "question", "Review the change")),
            &fixture.ctx,
        )
        .await;
    timeout(Duration::from_secs(3), next(&mut replies, "chat.reply"))
        .await
        .unwrap();
    let request = fixture
        .service
        .confirmations()
        .list("account", "web-fixture-user", Some(session.id.as_str()))
        .remove(0);
    assert_eq!(request.state, ConfirmationState::Pending);
    fixture
        .service
        .web_delete(
            &owner,
            &session.id,
            session.revision,
            &fixture.host.config.code_workspace,
        )
        .await
        .unwrap();
    fixture
        .host
        .panel_discard_session(&session.id, &fixture.ctx)
        .await
        .unwrap();
    assert!(fixture
        .service
        .confirmations()
        .get("account", "web-fixture-user", &request.id)
        .is_err());
    assert!(fixture.service.confirmations().deliveries().is_empty());
    fixture.host.panel_maintenance(&fixture.ctx).await;
    assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
    assert!(fixture
        .host
        .strict_history(&context.root_history_key())
        .await
        .is_err());
}
