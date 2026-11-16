use albert_core::sessions::{actor::ActorToolPolicy, SessionContext};
use octo_connector_telegram::{
    CommandMenuScope, PreflightDecision, TelegramHostPolicy, VerifiedActor,
};
use octo_core::{ChannelId, ConnectorId, Envelope, EventKind};
use sqlx::{query, query_scalar};
#[path = "common/account_login.rs"]
mod fixture;
use self::fixture::{accepted, admitted, bot, fixture, frame};

#[tokio::test]
async fn login_is_native_secretless_and_selected_conversation_survives_reauthentication() {
    let (_dir, service, policy) = fixture().await;
    assert!(matches!(
        policy.preflight(frame(1, 7, 7, "hello")).await.unwrap(),
        PreflightDecision::Consume { .. }
    ));
    let token = service
        .accounts()
        .issue("account", "telegram")
        .await
        .unwrap();
    let login = frame(2, 7, 7, &format!("/login {}", token.token));
    assert!(matches!(
        policy.preflight(login).await.unwrap(),
        PreflightDecision::Consume {
            delete_message: true,
            ..
        }
    ));
    let original = accepted(admitted(&policy, frame(3, 7, 7, "hello")).await);
    let context = original.context.get::<SessionContext>().unwrap();
    assert_eq!(context.user_id().unwrap().as_str(), "profile");
    assert_eq!(context.tool_policy(), ActorToolPolicy::ExistingHost);
    let selected = service.create_private(context, "Research").await.unwrap();
    assert!(matches!(
        policy.preflight(frame(4, 7, 7, "/logout")).await.unwrap(),
        PreflightDecision::Consume { .. }
    ));
    assert!(!context.is_live());
    assert!(matches!(
        policy.preflight(frame(5, 7, 7, "hello")).await.unwrap(),
        PreflightDecision::Consume { .. }
    ));
    let token = service
        .accounts()
        .issue("account", "telegram")
        .await
        .unwrap();
    policy
        .preflight(frame(6, 7, 7, &format!("/login {}", token.token)))
        .await
        .unwrap();
    let reopened = accepted(admitted(&policy, frame(7, 7, 7, "continue")).await);
    assert_eq!(
        reopened
            .context
            .get::<SessionContext>()
            .unwrap()
            .session_id(),
        selected.session_id()
    );
    let tokens: i64 = query_scalar("SELECT COUNT(*) FROM turns WHERE content LIKE '%albert-tg-%'")
        .fetch_one(service.database().pool())
        .await
        .unwrap();
    assert_eq!(tokens, 0);
}
#[tokio::test]
async fn group_guest_and_registered_actor_policies_follow_current_account() {
    let (_dir, service, policy) = fixture().await;
    let guest = accepted(admitted(&policy, frame(1, 8, -42, "@fixture_bot hello")).await);
    let group = guest.context.get::<SessionContext>().unwrap();
    assert_eq!(group.tool_policy(), ActorToolPolicy::NoTools);
    assert!(!group.is_private());
    assert!(matches!(
        policy
            .preflight(frame(2, 8, -42, "unaddressed"))
            .await
            .unwrap(),
        PreflightDecision::Consume { notice: None, .. }
    ));
    assert!(matches!(
        policy.preflight(frame(3, 8, -42, "/cancel")).await.unwrap(),
        PreflightDecision::Consume { .. }
    ));
    let token = service
        .accounts()
        .issue("account", "telegram")
        .await
        .unwrap();
    policy
        .preflight(frame(4, 7, 7, &format!("/login {}", token.token)))
        .await
        .unwrap();
    let owner = accepted(admitted(&policy, frame(5, 7, -42, "@fixture_bot research")).await);
    let owner = owner.context.get::<SessionContext>().unwrap();
    assert_eq!(owner.tool_policy(), ActorToolPolicy::ExistingHost);
    assert_eq!(owner.session_id(), group.session_id());
    assert!(matches!(
        policy
            .preflight(frame(6, 7, -42, "/sessions"))
            .await
            .unwrap(),
        PreflightDecision::Consume { .. }
    ));
    query("UPDATE albert_login_accounts SET role='user' WHERE id='account'")
        .execute(service.database().pool())
        .await
        .unwrap();
    let user = accepted(admitted(&policy, frame(7, 7, -42, "@fixture_bot hello")).await);
    assert_eq!(
        user.context.get::<SessionContext>().unwrap().tool_policy(),
        ActorToolPolicy::ChatOnly
    );
    assert!(matches!(
        policy.preflight(frame(8, 7, 7, "/restart")).await.unwrap(),
        PreflightDecision::Consume { .. }
    ));
    assert!(!owner.authorized().await);
    let menu = policy
        .command_menu(CommandMenuScope::Actor(VerifiedActor {
            bot: bot(),
            actor_id: 7,
            actor_is_bot: false,
            chat_id: 7,
            chat_type: "private".into(),
        }))
        .await
        .unwrap()
        .unwrap();
    assert!(menu.iter().any(|c| c.command == "sessions"));
    assert!(!menu.iter().any(|c| c.command == "restart"));
}
#[tokio::test]
async fn forwarded_group_and_embedded_credentials_are_consumed_before_admission() {
    let (_dir, service, policy) = fixture().await;
    let token = service
        .accounts()
        .issue("account", "telegram")
        .await
        .unwrap();
    let mut forwarded = frame(1, 7, 7, &format!("/login {}", token.token));
    forwarded.forwarded = true;
    for input in [
        forwarded,
        frame(2, 7, -42, &format!("/login {}", token.token)),
        frame(3, 7, 7, &format!("Please save {}", token.token)),
        frame(4, 7, 7, "123456789:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
    ] {
        assert!(matches!(
            policy.preflight(input).await.unwrap(),
            PreflightDecision::Consume {
                delete_message: true,
                ..
            }
        ));
    }
    assert!(service
        .accounts()
        .actor("telegram", 99, 7)
        .await
        .unwrap()
        .is_none());
}
#[tokio::test]
async fn web_report_delivery_requires_current_destination_binding_and_stops_on_logout() {
    let (_dir, service, policy) = fixture().await;
    let token = service
        .accounts()
        .issue("account", "telegram")
        .await
        .unwrap();
    policy
        .preflight(frame(1, 7, 7, &format!("/login {}", token.token)))
        .await
        .unwrap();
    let native = accepted(admitted(&policy, frame(2, 7, 7, "hello")).await);
    let web_grant = service.accounts().account("account").await.unwrap();
    let context = native
        .context
        .get::<SessionContext>()
        .unwrap()
        .clone()
        .with_account(web_grant.clone());
    let report = Envelope::new(
        ConnectorId::new("albert"),
        EventKind::new("chat.reply"),
        "Report from the web session",
    )
    .with_target(ConnectorId::new("telegram"))
    .with_channel(ChannelId::new("7"))
    .with_context(context);
    assert_eq!(
        policy.authorize_outbound(bot(), 7, &report).await.unwrap(),
        Some(true)
    );
    assert!(policy.allow_delivery(&report));
    assert_eq!(
        policy.authorize_outbound(bot(), 8, &report).await.unwrap(),
        Some(false)
    );
    service.accounts().logout("telegram", 99, 7).await.unwrap();
    assert!(web_grant.valid().await);
    assert!(!policy.allow_delivery(&report));
    assert_eq!(
        policy.authorize_outbound(bot(), 7, &report).await.unwrap(),
        Some(false)
    );
}
