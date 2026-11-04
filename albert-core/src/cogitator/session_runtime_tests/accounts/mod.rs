use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};

use octo_core::{CogitatorContext, ConnectorCapabilities, ConnectorId, ConnectorInfo, EventKind};
use rig::completion::Message;
use serde_json::{from_value, json};
use sqlx::query;
use tokio::time::{sleep, timeout};

use crate::{
    accounts::AccountGrant,
    cogitator::testing::sessions::{message, reply, Fixture},
    history::{tool_trace, HistoryStore},
    service::session::web::WebActor,
    sessions::{
        actor::ActorToolPolicy, Admission, ExternalIdentity, ReplyRoute, SessionContext, UserId,
    },
    subagents::{Args, SubagentTool},
};

async fn account(fixture: &Fixture) -> (SessionContext, AccountGrant) {
    let private = fixture.context("telegram", "7", "7").await;
    query("INSERT INTO albert_login_accounts(id,username,password_hash,role,canonical_user_id,must_change_password) VALUES ('account','fixture','unused','owner','test-owner',0)")
        .execute(fixture.service.database().pool()).await.unwrap();
    let accounts = fixture.service.accounts();
    accounts
        .register_bot("telegram", 99, "fixture_bot")
        .await
        .unwrap();
    let token = accounts.issue("account", "telegram").await.unwrap();
    let grant = accounts
        .login("telegram", 99, 7, 7, &token.token)
        .await
        .unwrap();
    (private.with_account(grant.clone()), grant)
}

#[tokio::test]
async fn native_logout_stops_its_root_and_child_but_preserves_independent_web_work() {
    let mut fixture = Fixture::with_model("account-hold").await;
    let mut connectors = fixture.ctx.connectors().to_vec();
    connectors.push(ConnectorInfo {
        id: ConnectorId::new("web"),
        capabilities: ConnectorCapabilities {
            event_kinds_emit: vec![EventKind::new("chat.message")],
            ..Default::default()
        },
    });
    fixture.ctx =
        CogitatorContext::new(fixture.ctx.shutdown.clone(), fixture.ctx.bus(), connectors);
    let (native, _) = account(&fixture).await;
    let incoming = Arc::new(
        (*message("telegram", "7", "7", "Native task", false, true))
            .clone()
            .with_context(native.clone()),
    );
    fixture.host.clone().handle(incoming, &fixture.ctx).await;
    fixture.request_count(1).await;
    let conversation = ("session".into(), native.root_history_key());
    let child_tool = SubagentTool::new(
        Arc::downgrade(&fixture.host),
        fixture.ctx.clone(),
        conversation.clone(),
        "native-parent".into(),
        true,
    )
    .with_session(Some(native.clone()));
    let task = from_value(
        json!({"task":"Wait for a fact","models":["account-hold"],"connectors":[],"tools":[]}),
    )
    .unwrap();
    let spawned = fixture
        .host
        .subagent_command(&child_tool, Args::Spawn { task })
        .await
        .unwrap();
    let run = fixture
        .host
        .children
        .get(spawned["run_id"].as_str().unwrap(), &conversation, true)
        .unwrap();
    fixture.request_count(2).await;

    query("INSERT INTO albert_identity_bindings VALUES ('web','account','test-owner')")
        .execute(fixture.service.database().pool())
        .await
        .unwrap();
    let actor = WebActor {
        account_id: "account".into(),
        user_id: UserId::parse("test-owner").unwrap(),
        tools: ActorToolPolicy::ExistingHost,
    };
    let web_session = fixture
        .service
        .web_create(&actor, "Independent web task", false)
        .await
        .unwrap();
    let web_grant = fixture.service.accounts().account("account").await.unwrap();
    let web = fixture
        .service
        .web_context(&actor, &web_session.id)
        .await
        .unwrap()
        .with_account(web_grant.clone());
    let incoming = Arc::new(
        (*message(
            "web",
            "account",
            web.reply_route().channel(),
            "Web task",
            false,
            true,
        ))
        .clone()
        .with_context(web.clone()),
    );
    fixture.host.clone().handle(incoming, &fixture.ctx).await;
    fixture.request_count(3).await;

    fixture
        .service
        .accounts()
        .logout("telegram", 99, 7)
        .await
        .unwrap();
    timeout(Duration::from_secs(3), async {
        loop {
            if fixture
                .host
                .panel_active(native.session_id())
                .await
                .is_none()
                && !run.result.borrow().is_null()
            {
                break;
            }
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert!(!native.is_live());
    assert!(web_grant.valid().await);
    assert!(fixture.host.panel_active(web.session_id()).await.is_some());
    assert!(matches!(
        run.result.borrow()["outcome"]["status"].as_str(),
        Some("cancelled" | "closed")
    ));

    query("UPDATE albert_login_accounts SET disabled=1 WHERE id='account'")
        .execute(fixture.service.database().pool())
        .await
        .unwrap();
    fixture.service.accounts().invalidate_account("account");
    timeout(Duration::from_secs(3), async {
        while fixture.host.panel_active(web.session_id()).await.is_some() {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert!(!web.is_live());
    fixture.host.stop_turns(&fixture.ctx).await;
}

#[tokio::test]
async fn guest_cannot_interrupt_account_work_and_only_receives_public_group_context() {
    let fixture = Fixture::new().await;
    let (_, grant) = account(&fixture).await;
    let route = ReplyRoute::new("telegram", "-42", None).unwrap();
    let owner = fixture
        .service
        .admit(
            Admission::group(
                Some(ExternalIdentity::new("telegram", "7").unwrap()),
                route.clone(),
                "host",
            )
            .unwrap()
            .with_account(grant)
            .unwrap(),
        )
        .await
        .unwrap();
    let hidden = tool_trace(&[Message::user("PRIVATE-TOOL-CHECKPOINT-SENTINEL")]).unwrap();
    fixture
        .router
        .append(&owner.root_history_key(), &[hidden])
        .await
        .unwrap();
    let mut replies = fixture.replies().await;
    let incoming = Arc::new(
        (*message("telegram", "7", "-42", "Owner group task", true, true))
            .clone()
            .with_context(owner.clone()),
    );
    fixture
        .host
        .clone()
        .handle(incoming.clone(), &fixture.ctx)
        .await;
    fixture.request_count(1).await;
    let sequence = fixture.host.turn_seq.load(Ordering::Relaxed);
    let guest = fixture
        .service
        .admit(Admission::group(None, route, "host").unwrap())
        .await
        .unwrap()
        .without_tools();
    let question = Arc::new(
        (*message("telegram", "8", "-42", "Public group question", true, false))
            .clone()
            .with_context(guest),
    );
    fixture
        .host
        .clone()
        .handle(question.clone(), &fixture.ctx)
        .await;
    let busy = reply(&mut replies).await;
    assert_eq!(busy.correlation_id, Some(question.id));
    assert!(busy
        .payload_as::<String>()
        .unwrap()
        .contains("another request"));
    assert_eq!(fixture.host.turn_seq.load(Ordering::Relaxed), sequence);
    let completed = reply(&mut replies).await;
    assert_eq!(completed.correlation_id, Some(incoming.id));
    fixture
        .host
        .clone()
        .handle(question.clone(), &fixture.ctx)
        .await;
    fixture.request_count(2).await;
    let answer = reply(&mut replies).await;
    assert_eq!(answer.correlation_id, Some(question.id));
    let requests = fixture.server.requests.lock().unwrap();
    let guest_request = &requests[1];
    assert!(guest_request
        .get("tools")
        .and_then(|v| v.as_array())
        .is_none_or(|tools| tools.is_empty()));
    assert!(!guest_request["messages"]
        .to_string()
        .contains("PRIVATE-TOOL-CHECKPOINT-SENTINEL"));
    drop(requests);
    fixture.host.stop_turns(&fixture.ctx).await;
}
