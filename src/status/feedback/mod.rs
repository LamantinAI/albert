use octo_connector_telegram::TurnFeedback;

use super::StatusFeed;

impl StatusFeed {
    pub(crate) fn with_feedback(mut self, feedback: Option<TurnFeedback>) -> Self {
        self.feedback = feedback;
        self
    }

    pub(super) fn feedback_open(&self) -> bool {
        self.feedback.as_ref().is_none_or(TurnFeedback::is_open)
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use octo_connector_telegram::TurnFeedback;
    use octo_core::{ConnectorId, EventBus, Filter, InProcessBus, SubscribeOptions};
    use serde_json::json;
    use tokio::time::timeout;

    use crate::status::StatusFeed;

    #[tokio::test]
    async fn closing_presentation_preserves_the_work_checkpoint_and_actions() {
        let bus = Arc::new(InProcessBus::new(32));
        let mut events = bus
            .subscribe(Filter::by_kind("chat.status"), SubscribeOptions::default())
            .await
            .unwrap();
        let token = TurnFeedback::new("old-feedback");
        let feed = StatusFeed::new(
            bus,
            ConnectorId::new("albert"),
            ConnectorId::new("telegram"),
            None,
        )
        .with_feedback(Some(token.clone()));
        feed.start_external("call", "search", json!({"query":"evidence"}));
        feed.finish_external("call", "kept evidence");
        feed.progress("before cancellation").await;
        let emitted = timeout(Duration::from_secs(1), events.next())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            emitted.context.get::<TurnFeedback>().unwrap().id(),
            token.id()
        );
        feed.actions.lock().unwrap().push("kept action".into());
        token.close();
        feed.progress("late progress").await;
        assert!(timeout(Duration::from_millis(30), events.next())
            .await
            .is_err());
        assert!(feed.is_live());
        assert!(!feed.checkpoint().is_empty());
        assert_eq!(feed.drain_actions(), ["kept action"]);
    }
}
