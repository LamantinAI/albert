#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use octo_connector_telegram::{TurnFeedback, TURN_END};
    use octo_core::{ConnectorId, EventBus, Filter, SubscribeOptions};
    use tokio::time::timeout;

    use crate::{cogitator::messaging::fixture::Fixture, messaging::store::WorkState};

    #[tokio::test]
    async fn only_running_message_work_can_reattach_and_finish_its_current_presentation() {
        let mut fixture = Fixture::new("never-message-reattach").await;
        Arc::get_mut(&mut fixture.host)
            .unwrap()
            .config
            .stream_status = true;
        let mut events = fixture
            .ctx
            .bus()
            .subscribe(
                Filter::by_target(ConnectorId::new("telegram")),
                SubscribeOptions::default(),
            )
            .await
            .unwrap();
        let id = fixture
            .question("A bounded question, not a native human input")
            .await;
        fixture
            .host
            .open_presentation(&fixture.b, &fixture.ctx)
            .await; // Still queued.
        assert!(timeout(Duration::from_millis(20), events.next())
            .await
            .is_err());
        fixture.host.messaging_tick(&fixture.ctx).await;
        fixture.wait_requests(1).await;
        fixture
            .host
            .open_presentation(&fixture.b, &fixture.ctx)
            .await;
        let typing = timeout(Duration::from_secs(1), events.next())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(typing.kind.as_str(), "chat.typing");
        let token = typing.context.get::<TurnFeedback>().unwrap().clone();
        assert!(token.is_open());
        let request = fixture.server.requests.lock().unwrap()[0].clone();
        assert!(request["tools"]
            .as_array()
            .is_none_or(|tools| tools.is_empty()));
        assert!(timeout(Duration::from_secs(1), events.next())
            .await
            .unwrap()
            .unwrap()
            .payload_as::<String>()
            .unwrap()
            .contains("still running"));
        let a = fixture
            .service
            .open_private(
                &fixture.b,
                fixture.a.session_id(),
                fixture.b.selector_revision().unwrap(),
            )
            .await
            .unwrap();
        let detached = fixture
            .service
            .presentations()
            .event_for(&fixture.b, &a)
            .unwrap();
        fixture
            .host
            .detach_presentation(&detached, &fixture.ctx)
            .await;
        let ended = timeout(Duration::from_secs(1), events.next())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ended.kind.as_str(), TURN_END);
        assert!(!token.is_open());
        let back = fixture
            .service
            .open_private(&a, fixture.b.session_id(), a.selector_revision().unwrap())
            .await
            .unwrap();
        fixture.host.open_presentation(&back, &fixture.ctx).await;
        let attached = timeout(Duration::from_secs(1), events.next())
            .await
            .unwrap()
            .unwrap();
        let fresh = attached.context.get::<TurnFeedback>().unwrap().clone();
        assert_ne!(fresh.id(), token.id());
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
        fixture.host.stop_turns(&fixture.ctx).await;
        assert!(!fresh.is_open());
        fixture.host.open_presentation(&back, &fixture.ctx).await;
        assert_ne!(fixture.status(&id).await.work, Some(WorkState::Running));
    }
}
