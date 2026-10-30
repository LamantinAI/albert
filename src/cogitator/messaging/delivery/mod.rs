mod envelope;

use std::sync::atomic::Ordering;

use chrono::Utc;
use octo_core::{CogitatorContext, EventBus};
use tracing::warn;

use self::envelope::report_envelope;
use super::{routes::RuntimeRoutes, MessagingRuntime};
use crate::{
    cogitator::AlbertCogitator,
    messaging::{
        contract::{MessageIntent, ReturnAddress},
        store::{DeliveryFailure, DeliveryObservation},
        transport::{AdapterKind, ReportAttempt},
    },
    sessions::SessionContext,
};

impl AlbertCogitator {
    pub(in crate::cogitator::messaging) async fn deliver_reports(
        &self,
        runtime: &MessagingRuntime,
        ctx: &CogitatorContext,
    ) {
        let Some(service) = &self.session_service else {
            return;
        };
        let now = Utc::now().timestamp_millis();
        let _ = runtime.store.expire_delivery_observations(now).await;
        let after = runtime.delivery_cursor.lock().unwrap().clone();
        let Ok(ids) = runtime
            .store
            .delivery_page(after.as_deref(), runtime.settings.batch_size)
            .await
        else {
            return;
        };
        *runtime.delivery_cursor.lock().unwrap() = ids.last().map(|id| id.as_str().to_owned());
        for id in ids {
            let Ok((actor, target)) = runtime.store.resolved_delivery(&id).await else {
                continue;
            };
            let Some(route) = target.display.clone() else {
                continue;
            };
            let Some(adapter) = service.delivery_adapters().get(&route.connector) else {
                let _ = runtime
                    .store
                    .fail_delivery(&id, DeliveryFailure::Unsupported)
                    .await;
                continue;
            };
            if adapter == AdapterKind::Console && route.channel != "stdin" {
                let _ = runtime
                    .store
                    .fail_delivery(&id, DeliveryFailure::Unsupported)
                    .await;
                continue;
            }
            let address = ReturnAddress {
                session: target.session.clone(),
                display: route.clone(),
            };
            let context = match service
                .messaging_context(&actor, &target.session, &address)
                .await
            {
                Ok(context) => context,
                Err(_) => {
                    let _ = runtime
                        .store
                        .fail_delivery(&id, DeliveryFailure::Unavailable)
                        .await;
                    continue;
                }
            };
            let record = match runtime.store.received_record(&context, &id).await {
                Ok(record) => record,
                Err(_) => {
                    let _ = runtime
                        .store
                        .fail_delivery(&id, DeliveryFailure::Unavailable)
                        .await;
                    continue;
                }
            };
            let implicit = record.envelope.intent == MessageIntent::Response;
            let active_only = implicit && adapter == AdapterKind::Telegram;
            if active_only {
                match service.refresh_presentation(&context).await {
                    Ok(true) if service.presentations().active(&context) => {}
                    Ok(true) => {
                        let _ = runtime
                            .store
                            .reject_inactive_before_publish(&id, None)
                            .await;
                        continue;
                    }
                    Ok(false) | Err(_) => {
                        let _ = runtime
                            .store
                            .fail_delivery(&id, DeliveryFailure::Unavailable)
                            .await;
                        continue;
                    }
                }
            }
            let attempt = match runtime
                .store
                .begin_delivery(
                    &id,
                    &RuntimeRoutes::new(ctx),
                    now,
                    adapter.as_str(),
                    now.saturating_add(runtime.settings.delivery_observation_timeout_ms as i64),
                )
                .await
            {
                Ok(attempt) => attempt,
                Err(_) => {
                    let _ = runtime
                        .store
                        .fail_delivery(&id, DeliveryFailure::Unavailable)
                        .await;
                    continue;
                }
            };
            let route = attempt
                .target
                .display
                .clone()
                .expect("delivery start pins a display route");
            let receipt = ReportAttempt::new(
                runtime.store.clone(),
                id.clone(),
                attempt.token.clone(),
                adapter,
                route.connector.clone(),
                route.channel.clone(),
                runtime.settings.max_delivery_observations,
            );
            let envelope = report_envelope(&self.self_source, context, &record, &receipt);
            if active_only {
                let context = envelope
                    .context
                    .get::<SessionContext>()
                    .expect("fresh recipient context");
                if !service.presentations().active(context) {
                    let _ = runtime
                        .store
                        .reject_inactive_before_publish(&id, Some(&receipt.token))
                        .await;
                    continue;
                }
            }
            // No DB transaction, lifecycle lease or Session gate spans publish.
            // Bus acceptance never proves transport completion.
            if let Err(error) = ctx.bus().publish(envelope).await {
                warn!(%error, message_id=id.as_str(), "report publish outcome is uncertain");
                receipt
                    .record(DeliveryObservation::PublishUnknown, vec![])
                    .await;
            }
        }
        let previous = runtime.unknown_cleaned_at.load(Ordering::Relaxed);
        if now - previous >= 60_000
            && runtime
                .unknown_cleaned_at
                .compare_exchange(previous, now, Ordering::SeqCst, Ordering::Relaxed)
                .is_ok()
        {
            if let Err(error) = runtime
                .store
                .cleanup_expired_unknown(
                    now,
                    runtime.settings.unknown_retention_secs as i64 * 1000,
                    runtime.settings.batch_size,
                )
                .await
            {
                warn!(%error, "expired delivery uncertainty could not be archived");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use octo_approval::Ingress;
    use octo_connector_telegram::{
        DeliveryReport, DeliveryStatus, TelegramHostPolicy, TelegramOutputScope,
    };
    use octo_core::{
        ChannelId, ConnectorId, Envelope, EventBus, EventKind, Filter, SubscribeOptions,
    };
    use octo_history::{SessionStore, SessionUpdate};
    use serde_json::{from_value, json};
    use tokio::time::{sleep, timeout};

    use crate::{
        cogitator::messaging::{fixture::Fixture, LogicalWork},
        history::assistant_turn,
        messaging::{
            contract::{MessageId, WorkRef},
            store::{DeliveryState, ReportAdmission, WorkState},
            transport::ReportAttempt,
        },
        sessions::{telegram::SessionTelegramPolicy, SessionContext},
    };

    #[tokio::test]
    async fn selected_native_final_has_one_full_body_and_fresh_attempt_scope() {
        let fixture = Fixture::new("message-native-delivery").await;
        let active = fixture
            .service
            .open_private(
                &fixture.b,
                fixture.a.session_id(),
                fixture.b.selector_revision().unwrap(),
            )
            .await
            .unwrap();
        let mut replies = fixture
            .ctx
            .bus()
            .subscribe(Filter::by_kind("chat.reply"), SubscribeOptions::default())
            .await
            .unwrap();
        fixture
            .native(
                &active,
                &format!(
                    "Route final result POLICY_TARGET:{}",
                    fixture.b.session_id()
                ),
            )
            .await;
        let receipt = timeout(Duration::from_secs(3), replies.next())
            .await
            .unwrap()
            .unwrap();
        let text = receipt.payload_as::<String>().unwrap();
        assert!(
            text.contains("queued for the selected destination"),
            "{text}"
        );
        assert!(!text.contains("answer from message-native-delivery"));
        assert!(receipt.context.get::<ReportAttempt>().is_none());
        for _ in 0..3 {
            fixture.host.messaging_tick(&fixture.ctx).await;
        }
        let outbound = timeout(Duration::from_secs(3), replies.next())
            .await
            .unwrap()
            .unwrap();
        let attempt = outbound.context.get::<ReportAttempt>().unwrap();
        assert!(outbound
            .payload_as::<String>()
            .unwrap()
            .contains("answer from message-native-delivery"));
        assert!(outbound.context.get::<Ingress>().is_none());
        assert_eq!(
            outbound
                .context
                .get::<SessionContext>()
                .unwrap()
                .session_id(),
            fixture.b.session_id()
        );
        let scope = outbound.context.get::<TelegramOutputScope>().unwrap();
        assert!(!scope.scope.ephemeral);
        assert!(scope.scope.cleanup_scope.is_none());
        assert_eq!(scope.attempt_id.as_deref(), Some(attempt.token.as_str()));
        let status = fixture.status(&attempt.message).await;
        assert_eq!(status.delivery, DeliveryState::Started);
        let policy =
            SessionTelegramPolicy::new(fixture.service.clone(), fixture.host.authority.clone());
        policy.delivered(
            &outbound,
            &DeliveryReport {
                scope: Some(scope.clone()),
                chat_id: 7,
                message_ids: vec![41],
                status: DeliveryStatus::Sent,
            },
        );
        timeout(Duration::from_secs(2), async {
            loop {
                if !fixture
                    .status(&attempt.message)
                    .await
                    .physical
                    .observations
                    .is_empty()
                {
                    break;
                }
                sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            fixture.status(&attempt.message).await.delivery,
            DeliveryState::Started
        );
        policy.delivered(
            &outbound,
            &DeliveryReport {
                scope: Some(scope.clone()),
                chat_id: 7,
                message_ids: vec![],
                status: DeliveryStatus::Unknown,
            },
        );
        timeout(Duration::from_secs(2), async {
            loop {
                if fixture.status(&attempt.message).await.delivery == DeliveryState::Unknown {
                    break;
                }
                sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        assert!(fixture
            .host
            .messaging
            .as_ref()
            .unwrap()
            .store
            .delivery_page(None, 16)
            .await
            .unwrap()
            .is_empty());
        let source = fixture
            .service
            .history()
            .load_session(&fixture.a.journal())
            .await
            .unwrap();
        assert!(source.iter().any(|turn| turn
            .content
            .starts_with("answer from message-native-delivery")));
    }

    #[tokio::test]
    async fn actual_message_result_goes_to_selected_c_and_correlated_a_without_forwarding_model() {
        let fixture = Fixture::new("healthy").await;
        let c = fixture
            .service
            .create_private(&fixture.b, "C")
            .await
            .unwrap();
        let id = fixture.question("Compute actual B result").await;
        let configured = fixture.action(json!({"action":"reports","message_id":id,"policy":{"revision":1,"intermediate":null,"final_result":{"kind":"fixed_session","session":c.session_id()}}})).await;
        assert_eq!(configured["policy"]["revision"], 1, "{configured}");
        fixture.host.messaging_tick(&fixture.ctx).await;
        assert_eq!(
            fixture.wait_terminal(&id).await.work,
            Some(WorkState::Completed)
        );
        for _ in 0..5 {
            fixture.host.messaging_tick(&fixture.ctx).await;
        }
        let a = fixture
            .service
            .history()
            .load_session(&fixture.a.journal())
            .await
            .unwrap();
        let c = fixture
            .service
            .history()
            .load_session(&c.journal())
            .await
            .unwrap();
        assert!(a.iter().any(|turn| turn.content.contains("received.v1")
            && turn.content.contains("answer from healthy")));
        assert!(c.iter().any(|turn| turn.content.contains("received.v1")
            && turn.content.contains("answer from healthy")));
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
        assert_eq!(
            fixture
                .status(&id)
                .await
                .report_outcomes
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert!(fixture
            .host
            .messaging
            .as_ref()
            .unwrap()
            .store
            .message_report_policy(&fixture.source(&fixture.a), &id)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn cancelled_work_routes_selected_final_failure_and_clears_policy() {
        let fixture = Fixture::new("healthy").await;
        let c = fixture
            .service
            .create_private(&fixture.b, "C")
            .await
            .unwrap();
        let id = fixture.question("Do not start cancelled work").await;
        fixture.action(json!({"action":"reports","message_id":id,"policy":{"revision":1,"intermediate":null,"final_result":{"kind":"fixed_session","session":c.session_id()}}})).await;
        fixture
            .action(json!({"action":"cancel","message_id":id}))
            .await;
        for _ in 0..5 {
            fixture.host.messaging_tick(&fixture.ctx).await;
        }
        assert_eq!(fixture.status(&id).await.work, Some(WorkState::Cancelled));
        let records = fixture
            .service
            .history()
            .load_session(&c.journal())
            .await
            .unwrap();
        assert!(records
            .iter()
            .any(|turn| turn.content.contains("received.v1")
                && turn.content.contains("ended with status cancelled")));
        assert!(fixture.server.requests.lock().unwrap().is_empty());
        assert!(fixture
            .host
            .messaging
            .as_ref()
            .unwrap()
            .store
            .message_report_policy(&fixture.source(&fixture.a), &id)
            .await
            .unwrap()
            .is_none());
    }
    #[tokio::test]
    async fn native_optional_route_failure_retains_useful_source_final_and_clears_policy() {
        let fixture = Fixture::new("healthy").await;
        fixture.action(json!({"action":"reports","policy":{"revision":1,"intermediate":null,"final_result":{"kind":"fixed_session","session":fixture.b.session_id()}}})).await;
        fixture
            .service
            .history()
            .update_session(
                fixture.b.session_id(),
                SessionUpdate {
                    expected_revision: fixture.b.session().revision,
                    title: None,
                    archived: Some(true),
                },
            )
            .await
            .unwrap();
        let mut incoming = Envelope::new(
            ConnectorId::new("telegram"),
            EventKind::new("chat.message"),
            "native work".to_owned(),
        )
        .with_channel(ChannelId::new("7"));
        incoming.context.insert(fixture.a.clone());
        incoming
            .context
            .insert(LogicalWork(WorkRef::Turn("native-existing-work".into())));
        let outcome = fixture
            .host
            .commit_native_report(
                &incoming,
                "native-attempt",
                vec![assistant_turn("Useful completed source result".into())],
                "Useful completed source result",
                &fixture.ctx,
            )
            .await
            .unwrap();
        assert!(matches!(
            outcome,
            Some(ReportAdmission::Failed {
                code: "unavailable",
                ..
            })
        ));
        let source = fixture
            .service
            .history()
            .load_session(&fixture.a.journal())
            .await
            .unwrap();
        assert!(source
            .iter()
            .any(|turn| turn.content == "Useful completed source result"));
        assert!(fixture
            .host
            .messaging
            .as_ref()
            .unwrap()
            .store
            .report_policy(&fixture.source(&fixture.a))
            .await
            .unwrap()
            .is_none());
        assert!(fixture.server.requests.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn temporary_active_final_route_requires_save_without_losing_native_result() {
        let fixture = Fixture::new("healthy").await;
        fixture.action(json!({"action":"reports","policy":{"revision":1,"intermediate":null,"final_result":{"kind":"channel_active","route":{"connector":"telegram","channel":"7"}}}})).await;
        let _temporary = fixture
            .service
            .start_temporary(&fixture.b, None)
            .await
            .unwrap();
        let mut incoming = Envelope::new(
            ConnectorId::new("telegram"),
            EventKind::new("chat.message"),
            "native work".to_owned(),
        )
        .with_channel(ChannelId::new("7"));
        incoming.context.insert(fixture.a.clone());
        incoming
            .context
            .insert(LogicalWork(WorkRef::Turn("native-existing-work".into())));
        let outcome = fixture
            .host
            .commit_native_report(
                &incoming,
                "native-temp-target",
                vec![assistant_turn("Useful permanent source result".into())],
                "Useful permanent source result",
                &fixture.ctx,
            )
            .await
            .unwrap();
        assert!(matches!(
            outcome,
            Some(ReportAdmission::Failed {
                code: "save_required",
                ..
            })
        ));
        let source = fixture
            .service
            .history()
            .load_session(&fixture.a.journal())
            .await
            .unwrap();
        assert!(source
            .iter()
            .any(|turn| turn.content == "Useful permanent source result"));
    }
    #[tokio::test]
    async fn fast_recipient_observes_policy_atomically_bound_by_single_send_action() {
        let fixture = Fixture::new("healthy").await;
        let c = fixture
            .service
            .create_private(&fixture.b, "Atomic final destination")
            .await
            .unwrap();
        let sent = fixture.action(json!({"action":"send","destination":{"kind":"fixed_session","session":fixture.b.session_id()},"intent":"question","content":"Complete immediately with final result to C","policy":{"revision":1,"intermediate":null,"final_result":{"kind":"fixed_session","session":c.session_id()}}})).await;
        assert_eq!(sent["accepted"], true);
        assert_eq!(sent["report_policy_bound"], true);
        let id: MessageId = from_value(sent["message_id"].clone()).unwrap();
        fixture.host.messaging_tick(&fixture.ctx).await;
        assert_eq!(
            fixture.wait_terminal(&id).await.work,
            Some(WorkState::Completed)
        );
        for _ in 0..5 {
            fixture.host.messaging_tick(&fixture.ctx).await;
        }
        let records = fixture
            .service
            .history()
            .load_session(&c.journal())
            .await
            .unwrap();
        assert!(records
            .iter()
            .any(|turn| turn.content.contains("received.v1")
                && turn.content.contains("answer from healthy")));
        let status = fixture.status(&id).await;
        assert_eq!(status.intended.admission_report_policy.unwrap().revision, 1);
        assert_eq!(status.report_outcomes.unwrap().as_array().unwrap().len(), 2);
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
    }
}
