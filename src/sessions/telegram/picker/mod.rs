//! Native Telegram picker controls. The only retained state is opaque RAM tokens.
mod callback;
mod config;
pub(super) mod registry;
mod render;
pub(crate) use config::PickerSettings;

use std::sync::Mutex;

use self::registry::Registry;

pub(super) struct Picker {
    registry: Mutex<Registry>,
}
impl Picker {
    pub fn new(settings: PickerSettings) -> Self {
        Self {
            registry: Mutex::new(Registry::new(settings)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use octo_approval::Ingress;
    use octo_connector_telegram::{
        CallbackFrame, CallbackMessage, IngressDecision, InlineKeyboard, TelegramHostPolicy,
    };
    use octo_core::{ConnectorId, Envelope};
    use octo_history::{SessionId, SessionStore, SessionUpdate};
    use tokio::{spawn, task::yield_now, time::timeout};

    use crate::{
        history::HistoryStore,
        sessions::{
            presentation::{PresentationDetached, PresentationOpened},
            telegram::{
                tests::{accepted, admit, fixture},
                SessionTelegramPolicy,
            },
            SessionContext,
        },
    };

    fn frame(data: &str, query: &str) -> CallbackFrame {
        CallbackFrame {
            connector: ConnectorId::new("telegram"),
            bot_id: 99,
            update_id: 900,
            query_id: query.into(),
            actor_id: 7,
            message: Some(CallbackMessage {
                chat_id: 7,
                message_id: 500,
                chat_type: "private".into(),
                accessible: true,
                sender_id: Some(99),
            }),
            inline_message_id: None,
            data: data.into(),
        }
    }
    fn reply(decision: IngressDecision) -> Envelope {
        match decision {
            IngressDecision::Control {
                notice: None,
                mut post_events,
                ..
            } => {
                assert_eq!(post_events.len(), 1);
                post_events.pop().unwrap()
            }
            _ => panic!("expected keyboard postevent"),
        }
    }
    async fn current(policy: &SessionTelegramPolicy, index: u32) -> SessionContext {
        accepted(admit(policy, index, None).await)
            .context
            .get::<SessionContext>()
            .unwrap()
            .clone()
    }
    fn data(reply: &Envelope, label: &str) -> String {
        reply
            .context
            .get::<InlineKeyboard>()
            .unwrap()
            .rows()
            .iter()
            .flatten()
            .find(|button| button.label().contains(label))
            .unwrap()
            .data()
            .into()
    }
    fn callback_reply(events: Vec<Envelope>) -> Envelope {
        events
            .into_iter()
            .find(|event| event.kind.as_str() == "chat.reply")
            .unwrap()
    }

    #[tokio::test]
    async fn picker_opens_stable_named_target_from_current_view_and_replays_never_undo_navigation()
    {
        let (_dir, policy) = fixture().await;
        let a = current(&policy, 1).await;
        let b = policy
            .service
            .create_private(&a, "Исследование SQLite")
            .await
            .unwrap();
        let c = policy
            .service
            .create_private(&b, "Другая работа")
            .await
            .unwrap();
        let list = reply(admit(&policy, 2, Some("/sessions")).await);
        let button = data(&list, "Исследование SQLite");
        let (ack, _, events) = policy
            .callback(frame(&button, "q1"))
            .await
            .unwrap()
            .into_parts();
        assert!(ack.unwrap().contains("updated"));
        assert!(events
            .iter()
            .all(|event| event.context.get::<Ingress>().is_none()));
        assert!(events
            .iter()
            .any(|event| event.context.get::<PresentationDetached>().is_some()));
        assert!(events
            .iter()
            .any(|event| event.context.get::<PresentationOpened>().is_some()));
        let selected = current(&policy, 3).await;
        assert_eq!(selected.session_id(), b.session_id());
        assert_ne!(selected.selector_revision(), c.selector_revision());
        let later = policy
            .service
            .open_private(
                &selected,
                c.session_id(),
                selected.selector_revision().unwrap(),
            )
            .await
            .unwrap();
        let (_, _, events) = policy
            .callback(frame(&button, "q1"))
            .await
            .unwrap()
            .into_parts();
        assert!(events.is_empty());
        assert_eq!(current(&policy, 4).await.session_id(), later.session_id());
        policy.callback(frame(&button, "q2")).await.unwrap(); // New intent on the old list.
        assert_eq!(current(&policy, 5).await.session_id(), b.session_id());
        for context in [&a, &b, &c] {
            assert!(policy
                .service
                .journal_router()
                .load(&context.root_history_key())
                .await
                .is_empty());
        }
    }
    #[tokio::test]
    async fn next_page_button_appears_only_when_more_than_five_chats_remain() {
        let (_dir, policy) = fixture().await;
        let mut context = current(&policy, 1).await;
        for index in 0..4 {
            context = policy
                .service
                .create_private(&context, &format!("Chat {index}"))
                .await
                .unwrap();
        }
        let first = reply(admit(&policy, 2, Some("/sessions")).await);
        let keyboard = first.context.get::<InlineKeyboard>().unwrap();
        assert_eq!(keyboard.rows().len(), 5);
        assert!(!keyboard
            .rows()
            .iter()
            .flatten()
            .any(|button| button.label() == "Next page"));
        policy
            .service
            .create_private(&context, "Sixth chat")
            .await
            .unwrap();
        let next = reply(admit(&policy, 3, Some("/sessions")).await);
        let rows = next.context.get::<InlineKeyboard>().unwrap().rows();
        assert_eq!(rows.len(), 6);
        assert_eq!(rows.last().unwrap()[0].label(), "Next page");
    }

    #[tokio::test]
    async fn five_per_page_names_are_bounded_with_real_navigation_buttons_and_fresh_metadata() {
        let (_dir, policy) = fixture().await;
        let mut context = current(&policy, 1).await;
        let mut oldest: Option<SessionId> = None;
        for index in 0..7 {
            let title = if index < 2 {
                "Duplicate".into()
            } else {
                format!("Страница {index}\n{}\t", "очень длинное имя ".repeat(20))
            };
            context = policy
                .service
                .create_private(&context, &title)
                .await
                .unwrap();
            if index == 0 {
                oldest = Some(context.session_id().clone());
            }
        }
        let first = reply(admit(&policy, 2, Some("/sessions")).await);
        let keyboard = first.context.get::<InlineKeyboard>().unwrap();
        assert_eq!(keyboard.rows().len(), 6);
        assert!(keyboard
            .rows()
            .iter()
            .flatten()
            .all(|button| button.label().chars().count() <= 64
                && !button.label().chars().any(char::is_control)));
        assert!(keyboard
            .rows()
            .iter()
            .flatten()
            .any(|button| button.label().contains("(current)")));
        let oldest = oldest.unwrap();
        let catalog = policy.service.get(&context, &oldest).await.unwrap();
        policy
            .service
            .history()
            .update_session(
                &oldest,
                SessionUpdate {
                    expected_revision: catalog.revision,
                    title: Some("Renamed between pages".into()),
                    archived: None,
                },
            )
            .await
            .unwrap();
        let (_, _, events) = policy
            .callback(frame(&data(&first, "Next page"), "next"))
            .await
            .unwrap()
            .into_parts();
        let second = callback_reply(events);
        let buttons = second.context.get::<InlineKeyboard>().unwrap();
        assert_eq!(buttons.rows().len(), 4); // Three remaining Sessions + paging row.
        assert!(buttons
            .rows()
            .iter()
            .flatten()
            .any(|button| button.label().contains("Renamed between pages")));
        policy
            .service
            .create_private(&context, "CREATED AFTER SNAPSHOT")
            .await
            .unwrap();
        let (_, _, events) = policy
            .callback(frame(&data(&second, "Previous page"), "previous"))
            .await
            .unwrap()
            .into_parts();
        let previous = callback_reply(events);
        let rows = previous.context.get::<InlineKeyboard>().unwrap().rows();
        assert_eq!(rows.len(), 6);
        assert!(rows
            .iter()
            .flatten()
            .all(|button| !button.label().contains("CREATED AFTER SNAPSHOT")));
    }
    #[tokio::test]
    async fn unknown_restart_foreign_and_active_temporary_clicks_do_not_switch_or_create_history() {
        let (_dir, policy) = fixture().await;
        let original = current(&policy, 1).await;
        let b = policy
            .service
            .create_private(&original, "Target B")
            .await
            .unwrap();
        let list = reply(admit(&policy, 2, Some("/sessions")).await);
        let button = data(&list, "Target B");
        let mut foreign = frame(&button, "foreign");
        foreign.actor_id = 8;
        let (ack, _, events) = policy.callback(foreign).await.unwrap().into_parts();
        assert!(ack.unwrap().contains("/sessions"));
        assert!(events.is_empty());
        let restarted =
            SessionTelegramPolicy::new(policy.service.clone(), policy.authority.clone());
        let (ack, _, events) = restarted
            .callback(frame(&button, "restart"))
            .await
            .unwrap()
            .into_parts();
        assert!(ack.unwrap().contains("expired"));
        assert!(events.is_empty());
        let temporary = policy.service.start_temporary(&b, None).await.unwrap();
        let (ack, _, events) = policy
            .callback(frame(&button, "temporary"))
            .await
            .unwrap()
            .into_parts();
        assert!(ack.unwrap().contains("/end"));
        assert!(events.is_empty());
        assert_eq!(
            current(&policy, 3).await.session_id(),
            temporary.session_id()
        );
        assert!(policy
            .service
            .journal_router()
            .load(&b.root_history_key())
            .await
            .is_empty());
    }
    #[tokio::test]
    async fn cancellation_while_waiting_for_admission_keeps_pending_receipt_and_does_not_repeat() {
        let (_dir, policy) = fixture().await;
        let policy = Arc::new(policy);
        let a = current(&policy, 1).await;
        let b = policy
            .service
            .create_private(&a, "Pending target")
            .await
            .unwrap();
        let context = policy
            .service
            .open_private(&b, a.session_id(), b.selector_revision().unwrap())
            .await
            .unwrap();
        let list = reply(admit(&policy, 2, Some("/sessions")).await);
        let button = data(&list, "Pending target");
        let gate = policy.admission.lock().await;
        let owned = policy.clone();
        let requested = frame(&button, "pending");
        let task = spawn(async move { owned.callback(requested).await });
        timeout(Duration::from_secs(1), async {
            loop {
                if policy
                    .picker
                    .registry
                    .lock()
                    .unwrap()
                    .has_pending("pending")
                {
                    return;
                }
                yield_now().await;
            }
        })
        .await
        .unwrap();
        task.abort();
        let _ = task.await;
        drop(gate);
        let (ack, _, events) = policy
            .callback(frame(&button, "pending"))
            .await
            .unwrap()
            .into_parts();
        assert!(ack.unwrap().contains("not repeated"));
        assert!(events.is_empty());
        assert_eq!(current(&policy, 3).await.session_id(), context.session_id());
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use crate::sessions::{
        telegram::tests::{accepted, admit, fixture},
        SessionContext,
    };
    use octo_connector_telegram::{
        CallbackFrame, CallbackMessage, IngressDecision, InlineKeyboard, TelegramHostPolicy,
    };
    use octo_core::ConnectorId;
    use octo_history::{SessionStore, SessionUpdate};
    use sqlx::{query, query_scalar};

    #[tokio::test]
    async fn archived_target_and_missing_route_never_open_or_silently_repair_a_selector() {
        let (_dir, policy) = fixture().await;
        let a = accepted(admit(&policy, 1, None).await)
            .context
            .get::<SessionContext>()
            .unwrap()
            .clone();
        let b = policy
            .service
            .create_private(&a, "Archived target")
            .await
            .unwrap();
        let a = policy
            .service
            .open_private(&b, a.session_id(), b.selector_revision().unwrap())
            .await
            .unwrap();
        let list = match admit(&policy, 2, Some("/sessions")).await {
            IngressDecision::Control { post_events, .. } => post_events
                .into_iter()
                .find(|event| event.context.get::<InlineKeyboard>().is_some())
                .unwrap(),
            _ => panic!("picker"),
        };
        let data = list
            .context
            .get::<InlineKeyboard>()
            .unwrap()
            .rows()
            .iter()
            .flatten()
            .find(|button| button.label().contains("Archived target"))
            .unwrap()
            .data()
            .to_owned();
        let click = |query: &str| CallbackFrame {
            connector: ConnectorId::new("telegram"),
            bot_id: 99,
            update_id: 700,
            query_id: query.into(),
            actor_id: 7,
            message: Some(CallbackMessage {
                chat_id: 7,
                message_id: 17,
                chat_type: "private".into(),
                accessible: true,
                sender_id: Some(99),
            }),
            inline_message_id: None,
            data: data.clone(),
        };
        policy
            .service
            .history()
            .update_session(
                b.session_id(),
                SessionUpdate {
                    expected_revision: b.session().revision,
                    title: None,
                    archived: Some(true),
                },
            )
            .await
            .unwrap();
        let (ack, _, events) = policy
            .callback(click("archive"))
            .await
            .unwrap()
            .into_parts();
        assert!(ack.unwrap().contains("archived"));
        assert!(events.is_empty());
        let current = accepted(admit(&policy, 3, None).await);
        assert_eq!(
            current
                .context
                .get::<SessionContext>()
                .unwrap()
                .session_id(),
            a.session_id()
        );
        query("DELETE FROM albert_private_session_routes")
            .execute(policy.service.database().pool())
            .await
            .unwrap();
        let (_, _, events) = policy
            .callback(click("missing-route"))
            .await
            .unwrap()
            .into_parts();
        assert!(events.is_empty());
        let routes: i64 = query_scalar("SELECT COUNT(*) FROM albert_private_session_routes")
            .fetch_one(policy.service.database().pool())
            .await
            .unwrap();
        assert_eq!(routes, 0);
    }
}
