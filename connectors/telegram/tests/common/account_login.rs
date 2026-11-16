use albert_connector_telegram::SessionTelegramPolicy;
use albert_core::{service::session::SessionService, sessions::settings::SessionSettings};
use octo_approval::Authority;
use octo_connector_telegram::{
    IngressDecision, NativeFrame, PreflightDecision, PreflightFrame, TelegramHostPolicy,
    VerifiedBot,
};
use octo_core::{ChannelId, ChannelMetadata, ConnectorId, Envelope, EventKind};
use octo_history::SqliteHistory;
use sqlx::query;
use std::{sync::Arc, time::Duration};
use tempfile::{tempdir, TempDir};
pub(super) async fn fixture() -> (TempDir, Arc<SessionService>, SessionTelegramPolicy) {
    let dir = tempdir().unwrap();
    let history = Arc::new(
        SqliteHistory::open_retained(dir.path().join("history.db"))
            .await
            .unwrap(),
    );
    let service = Arc::new(
        SessionService::from_history(history, SessionSettings::default())
            .await
            .unwrap(),
    );
    query("INSERT INTO albert_users(id) VALUES ('profile')")
        .execute(service.database().pool())
        .await
        .unwrap();
    query("INSERT INTO albert_login_accounts(id,username,password_hash,role,canonical_user_id,must_change_password) VALUES ('account','fixture','unused','owner','profile',0)").execute(service.database().pool()).await.unwrap();
    let authority = Authority::new(Duration::from_secs(300), 128)
        .with_scope_validator(service.scope_registry());
    let policy = SessionTelegramPolicy::new(service.clone(), authority)
        .with_accounts(service.accounts().clone());
    policy.bot_ready(bot()).await.unwrap();
    (dir, service, policy)
}
pub(super) fn bot() -> VerifiedBot {
    VerifiedBot {
        connector: ConnectorId::new("telegram"),
        bot_id: 99,
        username: "fixture_bot".into(),
    }
}
pub(super) fn frame(index: u32, actor: i64, chat: i64, text: &str) -> PreflightFrame {
    PreflightFrame {
        bot: bot(),
        update_id: index,
        message_id: index as i32,
        date: 1_800_000_000,
        chat_id: chat,
        actor_id: Some(actor),
        chat_type: if chat > 0 { "private" } else { "supergroup" }.into(),
        actor_is_bot: false,
        sender_chat_id: None,
        forwarded: false,
        text: Some(text.into()),
        caption: None,
        command_text: text.starts_with('/').then(|| text.to_owned()),
        addressed: text.starts_with('@'),
    }
}
pub(super) async fn admitted(
    policy: &SessionTelegramPolicy,
    frame: PreflightFrame,
) -> IngressDecision {
    let native = NativeFrame {
        connector: frame.bot.connector.clone(),
        bot_id: frame.bot.bot_id,
        update_id: frame.update_id,
        message_id: frame.message_id,
        date: frame.date,
        chat_id: frame.chat_id,
        actor_id: frame.actor_id,
        chat_type: frame.chat_type.clone(),
        command_text: frame.command_text.clone(),
        digest: format!("message-{}", frame.update_id),
        media: false,
    };
    let input_text = frame.text.clone().unwrap();
    let PreflightDecision::Admit { context, role } = policy.preflight(frame).await.unwrap() else {
        panic!("Expected account preflight admission");
    };
    let mut envelope = Envelope::new(
        native.connector.clone(),
        EventKind::new("chat.message"),
        input_text,
    )
    .with_channel(ChannelId::new(native.chat_id.to_string()))
    .with_channel_metadata(
        ChannelMetadata::default()
            .with_tag("role", role.as_str())
            .with_tag("chat_type", native.chat_type.clone())
            .with_tag("sender_id", native.actor_id.unwrap().to_string())
            .with_tag("chat_id", native.chat_id.to_string()),
    );
    envelope.context = context;
    policy.admit(native, envelope).await.unwrap()
}
pub(super) fn accepted(decision: IngressDecision) -> Envelope {
    match decision {
        IngressDecision::Accept { envelope, .. } => envelope,
        _ => panic!("Expected accepted chat message"),
    }
}
