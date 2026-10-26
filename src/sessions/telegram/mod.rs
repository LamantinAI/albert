//! Adapter labels for supervised Telegram I/O; session ownership remains in
//! SessionService, independently of this transport's cleanup lifecycle.

mod commands;
mod policy;

pub(crate) use policy::SessionTelegramPolicy;

use octo_connector_telegram::{IngressScope, TelegramOutputScope};
use octo_history::SessionKind;

use super::{cleanup_scope_for, SessionContext};

pub(crate) fn output_scope(
    context: &SessionContext,
    attempt_id: Option<String>,
) -> TelegramOutputScope {
    let ephemeral = context.session().kind == SessionKind::Temporary;
    let cleanup_scope = context.cleanup_scope().map(str::to_owned).or_else(|| {
        ephemeral.then(|| cleanup_scope_for(context.reply_route(), context.session_id()))
    });
    TelegramOutputScope {
        scope: IngressScope {
            key: context.root_history_key(),
            epoch: context.scope_epoch().unwrap_or("permanent").into(),
            ephemeral,
            cleanup_scope,
        },
        attempt_id,
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use octo_approval::Authority;
    use octo_connector_telegram::{IngressDecision, NativeFrame, TelegramHostPolicy};
    use octo_core::{ChannelId, ConnectorId, Envelope, EventKind};
    use octo_history::{
        AppendRequest, HistoryStore, PageRequest, SessionStore, SqliteHistory, Turn,
    };
    use tempfile::{tempdir, TempDir};

    use super::{output_scope, SessionTelegramPolicy};
    use crate::{
        service::session::SessionService,
        sessions::{
            native::request_alias, settings::SessionSettings, SessionContext, SessionEnded,
        },
    };

    async fn fixture() -> (TempDir, SessionTelegramPolicy) {
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
        let authority = Authority::new(Duration::from_secs(300), 128)
            .with_scope_validator(service.scope_registry());
        (dir, SessionTelegramPolicy::new(service, authority))
    }

    fn frame(index: u32, command: Option<&str>) -> NativeFrame {
        NativeFrame {
            connector: ConnectorId::new("telegram"),
            bot_id: 99,
            update_id: index,
            message_id: index as i32,
            date: 1_800_000_000 + i64::from(index),
            chat_id: 7,
            actor_id: Some(7),
            chat_type: "private".into(),
            command_text: command.map(str::to_owned),
            digest: format!("body-{index}"),
            media: false,
        }
    }

    fn envelope(frame: &NativeFrame) -> Envelope {
        Envelope::new(
            frame.connector.clone(),
            EventKind::new("chat.message"),
            "input".to_owned(),
        )
        .with_channel(ChannelId::new(frame.chat_id.to_string()))
    }

    async fn admit(
        policy: &SessionTelegramPolicy,
        index: u32,
        command: Option<&str>,
    ) -> IngressDecision {
        let frame = frame(index, command);
        policy.admit(frame.clone(), envelope(&frame)).await.unwrap()
    }

    fn accepted(decision: IngressDecision) -> Envelope {
        match decision {
            IngressDecision::Accept { envelope, .. } => envelope,
            _ => panic!("expected accepted input"),
        }
    }

    fn notice(decision: IngressDecision) -> String {
        match decision {
            IngressDecision::Control {
                notice: Some(text), ..
            } => text,
            _ => panic!("expected control notice"),
        }
    }

    #[tokio::test]
    async fn private_controls_switch_before_next_input_and_never_echo_transcript() {
        let (_dir, policy) = fixture().await;
        let original = accepted(admit(&policy, 1, None).await);
        let original = original.context.get::<SessionContext>().unwrap().clone();
        policy
            .service
            .journal_router()
            .append(
                &original.root_history_key(),
                &[Turn::user("private old history")],
            )
            .await
            .unwrap();
        assert!(notice(admit(&policy, 2, Some("/new Research")).await).contains("New chat"));
        let research = accepted(admit(&policy, 3, None).await);
        let research = research.context.get::<SessionContext>().unwrap();
        assert_ne!(research.session_id(), original.session_id());
        assert_eq!(
            research.reply_route().connector(),
            original.reply_route().connector()
        );
        assert_eq!(
            research.reply_route().channel(),
            original.reply_route().channel()
        );
        let response = notice(
            admit(
                &policy,
                4,
                Some(&format!("/open {}", original.session_id())),
            )
            .await,
        );
        assert!(!response.contains("private old history"));
        let selected = accepted(admit(&policy, 5, None).await);
        assert_eq!(
            selected
                .context
                .get::<SessionContext>()
                .unwrap()
                .session_id(),
            original.session_id()
        );
    }

    #[tokio::test]
    async fn end_emits_native_close_token_and_late_messages_cannot_leak_into_original() {
        let (_dir, policy) = fixture().await;
        let original = accepted(admit(&policy, 1, None).await);
        let original = original.context.get::<SessionContext>().unwrap().clone();
        assert!(notice(admit(&policy, 2, Some("/tmp")).await).contains("Temporary chat"));
        let tmp = accepted(admit(&policy, 3, None).await);
        let context = tmp.context.get::<SessionContext>().unwrap().clone();
        assert_ne!(context.session_id(), original.session_id());
        policy
            .service
            .journal_router()
            .append(
                &context.root_history_key(),
                &[Turn::user("temporary secret")],
            )
            .await
            .unwrap();
        match admit(&policy, 5, Some("/end")).await {
            IngressDecision::Control {
                post_events,
                cleanup: Some(_),
                ..
            } => {
                assert_eq!(post_events.len(), 1);
                assert_eq!(
                    post_events[0]
                        .context
                        .get::<SessionEnded>()
                        .unwrap()
                        .session_id,
                    *context.session_id()
                );
            }
            _ => panic!("expected native closure"),
        }
        assert!(!policy.allow_publish(&tmp));
        assert!(!policy.allow_delivery(&tmp));
        assert!(notice(admit(&policy, 4, None).await).contains("ended"));
        let next = accepted(admit(&policy, 6, None).await);
        assert_eq!(
            next.context.get::<SessionContext>().unwrap().session_id(),
            original.session_id()
        );
        assert!(policy
            .service
            .journal_router()
            .strict_load(&original.root_history_key())
            .await
            .unwrap()
            .is_empty());
        assert!(matches!(
            admit(&policy, 2, Some("/tmp")).await,
            IngressDecision::Ignore
        ));
        assert!(matches!(
            admit(&policy, 5, Some("/end")).await,
            IngressDecision::Ignore
        ));
    }

    #[tokio::test]
    async fn save_then_end_keeps_inflight_work_live_and_detaches_late_outputs_from_cleanup() {
        let (_dir, policy) = fixture().await;
        let _ = admit(&policy, 1, Some("/tmp")).await;
        let tmp = accepted(admit(&policy, 2, None).await);
        let context = tmp.context.get::<SessionContext>().unwrap().clone();
        policy
            .service
            .journal_router()
            .append(&context.root_history_key(), &[Turn::user("save me")])
            .await
            .unwrap();
        let output = tmp
            .clone()
            .with_context(output_scope(&context, Some("attempt".into())));
        assert!(notice(admit(&policy, 3, Some("/save")).await).contains("Chat saved"));
        match admit(&policy, 5, Some("/end")).await {
            IngressDecision::Control {
                post_events,
                cleanup: Some(_),
                ..
            } => assert!(post_events.is_empty()),
            _ => panic!("expected navigation end"),
        }
        assert!(policy.allow_delivery(&output));
        let effective = policy.output_scope(&output).unwrap();
        assert!(!effective.scope.ephemeral);
        assert!(effective.scope.cleanup_scope.is_none());
        let delayed = accepted(admit(&policy, 4, None).await);
        assert_eq!(
            delayed
                .context
                .get::<SessionContext>()
                .unwrap()
                .session_id(),
            context.session_id()
        );
        assert_eq!(
            policy
                .service
                .history()
                .session_page(&context.journal(), PageRequest::default())
                .await
                .unwrap()
                .records
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn restart_blocks_unsaved_input_until_explicit_recovery() {
        let (_dir, policy) = fixture().await;
        let _ = admit(&policy, 1, Some("/tmp")).await;
        let restarted_service = Arc::new(
            SessionService::from_history(
                policy.service.history().clone(),
                SessionSettings::default(),
            )
            .await
            .unwrap(),
        );
        let restarted = SessionTelegramPolicy::new(restarted_service, policy.authority.clone());
        assert!(notice(admit(&restarted, 2, None).await).contains("ended"));
        assert!(notice(admit(&restarted, 3, Some("/end")).await).contains("discarded"));
        let _ = accepted(admit(&restarted, 4, None).await);
    }

    #[tokio::test]
    async fn group_selectors_are_denied_and_a_forwarded_command_is_only_input() {
        let (_dir, policy) = fixture().await;
        let mut group = frame(1, Some("/tmp"));
        group.chat_type = "supergroup".into();
        group.chat_id = -42;
        assert!(
            notice(policy.admit(group.clone(), envelope(&group)).await.unwrap())
                .contains("private channels")
        );
        group.command_text = None;
        let forwarded = accepted(policy.admit(group.clone(), envelope(&group)).await.unwrap());
        assert!(!forwarded
            .context
            .get::<SessionContext>()
            .unwrap()
            .is_private());
    }

    #[tokio::test]
    async fn native_receipt_replay_does_not_select_or_interrupt_a_different_chat() {
        let (_dir, policy) = fixture().await;
        let input = accepted(admit(&policy, 1, None).await);
        let context = input.context.get::<SessionContext>().unwrap();
        let alias = request_alias(&frame(1, None));
        let request = AppendRequest {
            namespace: alias.namespace.clone(),
            idempotency_key: alias.idempotency_key.clone(),
            digest: alias.digest.clone(),
            turns: vec![Turn::user("accepted")],
        };
        policy
            .service
            .journal_router()
            .append_aliased(&context.root_history_key(), &request, &[alias])
            .await
            .unwrap();
        let _ = admit(&policy, 2, Some("/new Other")).await;
        assert!(matches!(
            admit(&policy, 1, None).await,
            IngressDecision::Ignore
        ));
        let mut changed = frame(1, None);
        changed.digest = "changed".into();
        assert!(notice(
            policy
                .admit(changed.clone(), envelope(&changed))
                .await
                .unwrap()
        )
        .contains("conflicts"));
        let current = accepted(admit(&policy, 3, None).await);
        assert_ne!(
            current
                .context
                .get::<SessionContext>()
                .unwrap()
                .session_id(),
            context.session_id()
        );
    }

    #[tokio::test]
    async fn out_of_order_end_cannot_expose_an_already_admitted_temporary_frame() {
        let (_dir, policy) = fixture().await;
        let _ = admit(&policy, 10, Some("/tmp")).await;
        let input = accepted(admit(&policy, 30, None).await);
        let temporary = input.context.get::<SessionContext>().unwrap();
        assert!(notice(admit(&policy, 20, Some("/end")).await).contains("out of order"));
        assert!(temporary.is_live());
        let repeated = accepted(admit(&policy, 30, None).await);
        assert_eq!(
            repeated
                .context
                .get::<SessionContext>()
                .unwrap()
                .session_id(),
            temporary.session_id()
        );
        assert!(notice(admit(&policy, 40, Some("/end")).await).contains("discarded"));
        assert!(notice(admit(&policy, 30, None).await).contains("ended"));
    }
}
