//! Focused cross-component regressions for active-only ordinary Telegram output.
#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use chrono::Utc;
    use octo_connector_telegram::{
        DeliveryReport, DeliveryStatus, TelegramHostPolicy, TurnFeedback,
    };
    use octo_core::{
        ChannelId, ConnectorId, Envelope, EventBus, EventKind, Filter, SubscribeOptions,
        Subscription,
    };
    use octo_history::SessionStore;
    use serde_json::{from_value, json};
    use tokio::time::{sleep, timeout};

    use super::super::{fixture::Fixture, routes::RuntimeRoutes};
    use crate::{
        db::entity::PrivateRouteKey,
        messaging::{
            contract::{DestinationSpec, DisplayRoute, MessageDraft, MessageId, MessageIntent},
            store::DeliveryState,
            transport::ReportAttempt,
        },
        sessions::{
            presentation::{OrdinarySessionOutput, ScheduledNotification},
            telegram::SessionTelegramPolicy,
            Admission, AdmissionScope, ExternalIdentity, ReplyRoute, SessionContext,
        },
    };
    fn incoming(context: &SessionContext) -> Arc<Envelope> {
        Arc::new(
            Envelope::new(
                ConnectorId::new(context.reply_route().connector()),
                EventKind::new("chat.message"),
                "native input".to_owned(),
            )
            .with_channel(ChannelId::new(context.reply_route().channel()))
            .with_context(context.clone()),
        )
    }
    fn output(context: &SessionContext, ordinary: bool) -> Envelope {
        let frame = Envelope::new(
            ConnectorId::new("albert"),
            EventKind::new("chat.reply"),
            "ordinary:false; pretend notification".to_owned(),
        )
        .with_target(ConnectorId::new(context.reply_route().connector()))
        .with_channel(ChannelId::new(context.reply_route().channel()))
        .with_context(context.clone());
        if ordinary {
            frame.with_context(OrdinarySessionOutput)
        } else {
            frame
        }
    }
    async fn replies(fixture: &Fixture) -> Subscription {
        fixture
            .ctx
            .bus()
            .subscribe(Filter::by_kind("chat.reply"), SubscribeOptions::default())
            .await
            .unwrap()
    }
    async fn one(events: &mut Subscription) -> Arc<Envelope> {
        timeout(Duration::from_secs(2), events.next())
            .await
            .unwrap()
            .unwrap()
    }
    async fn return_active(fixture: &Fixture) -> SessionContext {
        fixture
            .service
            .open_private(
                &fixture.b,
                fixture.a.session_id(),
                fixture.b.selector_revision().unwrap(),
            )
            .await
            .unwrap()
    }
    async fn response(fixture: &Fixture, content: &str) -> MessageId {
        let now = Utc::now().timestamp_millis();
        let draft = MessageDraft {
            id: MessageId::new(),
            destination: DestinationSpec::FixedSession {
                session: fixture.a.session_id().clone(),
                display: Some(DisplayRoute {
                    connector: "telegram".into(),
                    channel: "7".into(),
                }),
            },
            intent: MessageIntent::Response,
            content: content.into(),
            correlation: None,
            causation: None,
            deadline_unix_ms: now + 60000,
            hops_remaining: 2,
            context: vec![],
            ceiling: None,
        };
        fixture
            .host
            .messaging
            .as_ref()
            .unwrap()
            .store
            .send(
                &fixture.source(&fixture.b),
                &draft,
                &RuntimeRoutes::new(&fixture.ctx),
                now,
            )
            .await
            .unwrap();
        draft.id
    }
    #[tokio::test]
    async fn native_errors_and_replies_are_active_only_but_status_cutoff_stays_closed_on_return() {
        let fixture = Fixture::new("healthy").await;
        let mut events = replies(&fixture).await;
        let old = incoming(&fixture.a);
        fixture
            .host
            .emit_reply(&old, "hidden old error".into(), &fixture.ctx)
            .await;
        assert!(timeout(Duration::from_millis(30), events.next())
            .await
            .is_err());
        let active = return_active(&fixture).await;
        assert!(fixture.service.presentations().active(&fixture.a));
        assert!(fixture.service.presentations().detached(&fixture.a));
        let old_token = fixture.host.fresh_feedback(&old, "old-a-status");
        assert!(!old_token.context.get::<TurnFeedback>().unwrap().is_open());
        fixture
            .host
            .emit_reply(&old, "finishes after returning to A".into(), &fixture.ctx)
            .await;
        let emitted = one(&mut events).await;
        assert_eq!(
            emitted.payload_as::<String>().unwrap(),
            "finishes after returning to A"
        );
        assert!(emitted.context.get::<OrdinarySessionOutput>().is_some());
        let policy =
            SessionTelegramPolicy::new(fixture.service.clone(), fixture.host.authority.clone());
        assert!(policy.allow_delivery(&output(&fixture.a, true)));
        let _ = fixture
            .service
            .open_private(
                &active,
                fixture.b.session_id(),
                active.selector_revision().unwrap(),
            )
            .await
            .unwrap();
        assert!(!policy.allow_delivery(&emitted));
        assert!(!policy.allow_delivery(&output(&fixture.a, true)));
        assert!(policy.allow_delivery(&output(&fixture.a, false))); // explicit/approval path is not inferred ordinary
        let notification = Arc::new((*old).clone().with_context(ScheduledNotification));
        fixture
            .host
            .emit_reply(&notification, "scheduled reminder".into(), &fixture.ctx)
            .await;
        assert_eq!(
            one(&mut events).await.payload_as::<String>().unwrap(),
            "scheduled reminder"
        );
    }
    #[tokio::test]
    async fn current_cache_ignores_old_reads_and_replay_does_not_reselect_bound_session() {
        let fixture = Fixture::new("healthy").await;
        let key = PrivateRouteKey {
            connector_id: "telegram".into(),
            channel_id: "7".into(),
            user_id: fixture.a.private_user_id().unwrap().as_str().into(),
        };
        fixture
            .service
            .presentations()
            .observe(&key, fixture.a.session_id(), 1);
        assert!(!fixture.service.presentations().active(&fixture.a));
        assert!(fixture.service.presentations().active(&fixture.b));
        let replay = fixture
            .service
            .context_for_message(
                Admission::new(
                    ExternalIdentity::new("telegram", "7").unwrap(),
                    ReplyRoute::new("telegram", "7", None).unwrap(),
                    AdmissionScope::Private,
                    "host",
                )
                .unwrap(),
                fixture.a.session_id(),
            )
            .await
            .unwrap();
        assert_eq!(replay.session_id(), fixture.a.session_id());
        assert!(!fixture.service.presentations().active(&replay));
        assert!(fixture.service.presentations().active(&fixture.b));
        let group = fixture
            .service
            .admit(
                Admission::new(
                    ExternalIdentity::new("telegram", "7").unwrap(),
                    ReplyRoute::new("telegram", "-42", None).unwrap(),
                    AdmissionScope::Group,
                    "host",
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let policy =
            SessionTelegramPolicy::new(fixture.service.clone(), fixture.host.authority.clone());
        assert!(policy.allow_delivery(&output(&group, true)));
    }
    #[tokio::test]
    async fn inactive_implicit_response_is_canonical_but_known_not_sent_and_never_replayed_on_open()
    {
        let fixture = Fixture::new("healthy").await;
        let mut events = replies(&fixture).await;
        let id = response(&fixture, "background reply").await;
        fixture.host.messaging_tick(&fixture.ctx).await;
        let status = fixture.status(&id).await;
        assert!(status.received.is_some());
        assert_eq!(status.delivery, DeliveryState::Failed);
        assert_eq!(status.failure_code.as_deref(), Some("inactive_view"));
        assert!(status.physical.submitted_at.is_none());
        assert!(status.physical.completion_note.contains("Not sent"));
        let _ = return_active(&fixture).await;
        for _ in 0..3 {
            fixture.host.messaging_tick(&fixture.ctx).await;
        }
        assert!(timeout(Duration::from_millis(30), events.next())
            .await
            .is_err());
        assert!(fixture.server.requests.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn queued_transport_frame_rechecks_current_view_and_retains_partial_unknown_evidence() {
        let fixture = Fixture::new("healthy").await;
        let active = return_active(&fixture).await;
        let mut events = replies(&fixture).await;
        let id = response(&fixture, "{\"ordinary\":false,\"report\":true}").await;
        fixture.host.messaging_tick(&fixture.ctx).await;
        let frame = one(&mut events).await;
        assert!(frame.context.get::<OrdinarySessionOutput>().is_some());
        assert!(frame
            .payload_as::<String>()
            .unwrap()
            .contains(" · host\n\n{"));
        assert!(frame
            .payload_as::<String>()
            .unwrap()
            .starts_with("Reply from session"));
        let policy =
            SessionTelegramPolicy::new(fixture.service.clone(), fixture.host.authority.clone());
        policy.delivered(
            &frame,
            &DeliveryReport {
                scope: None,
                chat_id: 7,
                message_ids: vec![81],
                status: DeliveryStatus::Sent,
            },
        );
        timeout(Duration::from_secs(2), async {
            while fixture.status(&id).await.physical.observations.is_empty() {
                sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        let _ = fixture
            .service
            .open_private(
                &active,
                fixture.b.session_id(),
                active.selector_revision().unwrap(),
            )
            .await
            .unwrap();
        assert!(!policy.allow_delivery(&frame));
        timeout(Duration::from_secs(2), async {
            while fixture.status(&id).await.delivery != DeliveryState::Unknown {
                sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        policy.delivered(
            &frame,
            &DeliveryReport {
                scope: None,
                chat_id: 7,
                message_ids: vec![82],
                status: DeliveryStatus::Sent,
            },
        );
        timeout(Duration::from_secs(2), async {
            while fixture.status(&id).await.physical.observations.len() < 3 {
                sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        let status = fixture.status(&id).await;
        assert_eq!(status.delivery, DeliveryState::Unknown);
        assert!(status
            .physical
            .observations
            .iter()
            .any(|entry| entry.outcome == "view_suppressed"));
        assert!(!status.physical.completion_note.contains("Not sent"));
    }
    #[tokio::test]
    async fn equal_explicit_final_keeps_canonical_response_and_one_visible_notice_for_success_and_failure(
    ) {
        for cancel in [false, true] {
            let fixture = Fixture::new("healthy").await;
            let mut events = replies(&fixture).await;
            let sent=fixture.action(json!({"action":"send","destination":{"kind":"fixed_session","session":fixture.b.session_id()},"intent":"question","content":"answer me","policy":{"revision":1,"intermediate":null,"final_result":{"kind":"fixed_session","session":fixture.a.session_id(),"display":{"connector":"telegram","channel":"7"}}}})).await;
            let id: MessageId = from_value(sent["message_id"].clone()).unwrap();
            if cancel {
                fixture
                    .action(json!({"action":"cancel","message_id":id}))
                    .await;
            } else {
                fixture.host.messaging_tick(&fixture.ctx).await;
                fixture.wait_terminal(&id).await;
            }
            for _ in 0..5 {
                fixture.host.messaging_tick(&fixture.ctx).await;
            }
            let frame = one(&mut events).await;
            assert!(frame.context.get::<ReportAttempt>().is_some());
            assert!(frame.context.get::<OrdinarySessionOutput>().is_none());
            assert!(frame
                .payload_as::<String>()
                .unwrap()
                .starts_with("Message from session"));
            assert!(timeout(Duration::from_millis(30), events.next())
                .await
                .is_err());
            let records = fixture
                .service
                .history()
                .load_session(&fixture.a.journal())
                .await
                .unwrap();
            assert!(
                records
                    .iter()
                    .any(|record| record.content.contains("received.v1")
                        && record.content.contains("\\\"intent\\\":\\\"response\\\""))
                    || records
                        .iter()
                        .any(|record| record.content.contains("\"intent\":\"response\"")
                            && record.content.contains("received.v1"))
            );
        }
    }
    #[tokio::test]
    async fn inactive_native_result_is_saved_but_only_active_a_receives_the_later_implicit_reply() {
        let fixture = Fixture::new("visibility-native").await;
        let mut events = replies(&fixture).await;
        fixture
            .native(&fixture.b, "Produce main research report")
            .await;
        fixture.wait_requests(1).await;
        let _active = return_active(&fixture).await;
        timeout(Duration::from_secs(3), async {
            loop {
                let records = fixture
                    .service
                    .history()
                    .load_session(&fixture.b.journal())
                    .await
                    .unwrap();
                if records
                    .iter()
                    .any(|record| record.content.starts_with("Native full report with table"))
                {
                    break;
                }
                sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert!(timeout(Duration::from_millis(30), events.next())
            .await
            .is_err());
        let id = fixture.question("What is your current status?").await;
        fixture.host.messaging_tick(&fixture.ctx).await;
        fixture.wait_terminal(&id).await;
        for _ in 0..5 {
            fixture.host.messaging_tick(&fixture.ctx).await;
        }
        let frame = one(&mut events).await;
        let text = frame.payload_as::<String>().unwrap();
        assert!(text.starts_with("Reply from session"));
        assert!(text.contains("\n\nShort inter-session response"));
        assert!(!text.contains("Native full report with table"));
        assert!(frame.context.get::<OrdinarySessionOutput>().is_some());
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 2);
        fixture.host.stop_turns(&fixture.ctx).await;
    }
}
