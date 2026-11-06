use std::{
    fs::{create_dir_all, write},
    sync::Arc,
    time::Duration,
};

use sqlx::query;
use tokio::time::timeout;

use super::{actor, configure_web, envelope, next, Fixture};
use crate::{
    composer::{CompactRequest, InputDigest, SkillSelection},
    sessions::{acceptance::SourceReceipt, actor::ActorToolPolicy},
    skills::SkillStore,
};

#[tokio::test]
async fn explicit_skill_is_loaded_before_inference_without_copying_its_body_to_history() {
    let mut fixture = Fixture::with_model("healthy").await;
    configure_web(&mut fixture);
    let directory = fixture._directory.path().join("skills");
    create_dir_all(directory.join("brief")).unwrap();
    write(directory.join("brief/SKILL.md"),"---\nname: Brief\ndescription: Focused answers\n---\nUse this unique selected instruction.").unwrap();
    Arc::get_mut(&mut fixture.host).unwrap().skills = SkillStore::load(directory, 4, 10, &[]);
    let owner = actor(&fixture, "account", ActorToolPolicy::ExistingHost).await;
    query("INSERT INTO albert_login_accounts(id,username,password_hash,role,canonical_user_id,must_change_password) VALUES ('account','fixture','unused','owner','web-fixture-user',0)").execute(fixture.service.database().pool()).await.unwrap();
    let session = fixture
        .service
        .web_create(&owner, "Skills fixture", false)
        .await
        .unwrap();
    let context = fixture
        .service
        .web_context(&owner, &session.id)
        .await
        .unwrap()
        .with_account(fixture.service.accounts().account("account").await.unwrap());
    let choice = fixture.host.panel_composer_skills().remove(0);
    let prepared = fixture
        .host
        .panel_prepare_skills(&[SkillSelection {
            id: choice.id,
            revision: choice.revision,
        }])
        .unwrap();
    let incoming =
        envelope(&context, "selected-request", "Question").with_context(prepared.clone());
    let mut replies = fixture.replies().await;
    fixture
        .host
        .clone()
        .handle(Arc::new(incoming), &fixture.ctx)
        .await;
    next(&mut replies, "chat.reply").await;
    assert!(fixture.server.requests.lock().unwrap()[0]["messages"]
        .to_string()
        .contains("Use this unique selected instruction."));
    let stored = fixture
        .router
        .strict_load(&context.root_history_key())
        .await
        .unwrap();
    assert!(!stored.iter().any(|turn| turn
        .content
        .contains("Use this unique selected instruction.")));
    assert!(stored.iter().any(|turn| turn.content.contains("Brief")));
    write(
        fixture._directory.path().join("skills/brief/SKILL.md"),
        "---\nname: Brief\n---\nChanged instructions",
    )
    .unwrap();
    let (receipt, accepted) = SourceReceipt::channel();
    fixture
        .host
        .clone()
        .handle(
            Arc::new(
                envelope(&context, "stale-request", "Stale selection")
                    .with_context(prepared)
                    .with_context(receipt),
            ),
            &fixture.ctx,
        )
        .await;
    assert!(timeout(Duration::from_secs(2), accepted)
        .await
        .unwrap()
        .unwrap()
        .is_err());
    assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
    assert_eq!(
        fixture
            .router
            .strict_load(&context.root_history_key())
            .await
            .unwrap()
            .len(),
        stored.len()
    );
    fixture.host.stop_turns(&fixture.ctx).await;
}

#[tokio::test]
async fn typed_compact_accepts_private_user_once_and_replay_does_not_restart_it() {
    let mut fixture = Fixture::with_model("healthy").await;
    configure_web(&mut fixture);
    let user = actor(&fixture, "account", ActorToolPolicy::ChatOnly).await;
    query("INSERT INTO albert_login_accounts(id,username,password_hash,role,canonical_user_id,must_change_password) VALUES ('account','fixture','unused','user','web-fixture-user',0)").execute(fixture.service.database().pool()).await.unwrap();
    let session = fixture
        .service
        .web_create(&user, "Compact fixture", false)
        .await
        .unwrap();
    let context = fixture
        .service
        .web_context(&user, &session.id)
        .await
        .unwrap()
        .with_account(fixture.service.accounts().account("account").await.unwrap());
    let mut replies = fixture.replies().await;
    fixture
        .host
        .clone()
        .handle(
            Arc::new(envelope(&context, "question", "Remember this decision")),
            &fixture.ctx,
        )
        .await;
    next(&mut replies, "chat.reply").await;
    let (receipt, accepted) = SourceReceipt::channel();
    let command = envelope(&context, "compact-request", "/compact Keep decisions")
        .with_context(CompactRequest {
            instructions: "Keep decisions".into(),
        })
        .with_context(InputDigest("command-digest".into()));
    fixture
        .host
        .clone()
        .handle(
            Arc::new(command.clone().with_context(receipt)),
            &fixture.ctx,
        )
        .await;
    assert!(
        !timeout(Duration::from_secs(2), accepted)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .duplicate
    );
    next(&mut replies, "chat.reply").await;
    let count = fixture.server.requests.lock().unwrap().len();
    let (receipt, accepted) = SourceReceipt::channel();
    fixture
        .host
        .clone()
        .handle(Arc::new(command.with_context(receipt)), &fixture.ctx)
        .await;
    assert!(
        timeout(Duration::from_secs(2), accepted)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .duplicate
    );
    assert_eq!(fixture.server.requests.lock().unwrap().len(), count);
    fixture.host.stop_turns(&fixture.ctx).await;
}
