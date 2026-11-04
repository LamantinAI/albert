//! Content-limited live progress. Execution output and provider reasoning never
//! enter this projection. Telegram retains its existing string status protocol.
use std::sync::Arc;

use chrono::Utc;
use octo_core::{ConnectorId, Envelope, EventBus, EventKind};
use rand::random;
use serde_json::{from_str, Value};
use tracing::warn;

use super::{clip, text::TextUpdate, StatusFeed};
use crate::{
    history::sessions::Backend, sessions::transport::TurnFeedback, status::domain::ProgressDelta,
    status::domain::ProgressItem,
};

pub(super) type StageSource = dyn Fn() -> Vec<(String, String, String)> + Send + Sync;

#[derive(Clone)]
pub struct ProgressUpdate(pub ProgressDelta);
pub(super) enum FeedUpdate {
    Progress(ProgressUpdate),
    Text(TextUpdate),
}

pub(super) fn item(id: String, kind: &str, label: &str, state: &str) -> ProgressItem {
    let at = Utc::now().to_rfc3339();
    ProgressItem {
        id,
        kind: kind.into(),
        label: label
            .chars()
            .filter(|c| !c.is_control())
            .take(160)
            .collect(),
        state: state.into(),
        started_at: at.clone(),
        updated_at: at,
        elapsed_ms: None,
    }
}

impl StatusFeed {
    pub(super) fn progress_tool_label(&self, name: &str, args: &str) -> String {
        if name == "scratchpad_goal" {
            if let Some(goal) = from_str::<Value>(args)
                .ok()
                .and_then(|value| value.get("goal").and_then(Value::as_str).map(str::to_owned))
            {
                let goal = goal.split_whitespace().collect::<Vec<_>>().join(" ");
                if !goal.is_empty() {
                    return format!("scratchpad goal “{}”", clip(&goal, 120));
                }
            }
        }
        if name == "dispatch_to_connector" {
            if let Some(target) = from_str::<Value>(args).ok().and_then(|value| {
                value
                    .get("target")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            }) {
                if self.connector_known(&target) {
                    return format!("Using {target}");
                }
            }
        }
        name.replace('_', " ")
    }
    pub(crate) fn with_stage_source(
        mut self,
        source: impl Fn() -> Vec<(String, String, String)> + Send + Sync + 'static,
    ) -> Self {
        self.stage_source = Some(Arc::new(source));
        self
    }

    pub(crate) async fn progress_phase(&self, label: &str) {
        self.progress_items(
            vec![item(
                format!("phase/{:032x}", random::<u128>()),
                "phase",
                label,
                "in_progress",
            )],
            None,
        )
        .await;
    }

    pub(crate) async fn progress_child(&self, id: &str, state: &str) {
        let state = match state {
            "running" | "completed" | "partial" | "failed" | "cancelled" | "timed_out" => state,
            _ => "unknown",
        };
        self.progress_items(
            vec![item(
                format!("child/{id}"),
                "child",
                "Delegated task",
                state,
            )],
            None,
        )
        .await;
    }

    pub(crate) async fn progress_stages(&self) {
        if let Some(source) = &self.stage_source {
            let items = source()
                .into_iter()
                .take(16)
                .map(|(id, label, state)| item(format!("stage/{id}"), "stage", &label, &state))
                .collect();
            self.progress_items(items, Some("stage")).await;
        }
    }

    pub(super) async fn progress_items(
        &self,
        items: Vec<ProgressItem>,
        replace_kind: Option<&str>,
    ) {
        let Some(binding) = self.presentation_binding() else {
            return;
        };
        let attempt = self
            .execution_attempt
            .clone()
            .or(binding.attempt)
            .or_else(|| binding.feedback.map(|feedback| feedback.id().to_owned()));
        let Some(attempt_id) = attempt else { return };
        let update = ProgressUpdate(ProgressDelta {
            attempt_id,
            at: Utc::now().to_rfc3339(),
            replace_kind: replace_kind.map(str::to_owned),
            items,
        });
        self.publish_line(
            "albert.progress",
            String::new(),
            Some(FeedUpdate::Progress(update)),
        )
        .await;
    }

    pub(super) async fn publish_line(
        &self,
        kind: &'static str,
        line: String,
        update: Option<FeedUpdate>,
    ) {
        let Some(binding) = self.presentation_binding() else {
            return;
        };
        if !self.is_live() || !binding.feedback.as_ref().is_none_or(TurnFeedback::is_open) {
            return;
        }
        let Some(feed) = &self.feed else { return };
        let lease = match &feed.backend {
            Some(backend) => Some(backend.read().await),
            None => None,
        };
        if lease
            .as_ref()
            .is_some_and(|backend| **backend == Backend::Closed)
        {
            return;
        }
        let mut envelope = Envelope::new(feed.source.clone(), EventKind::from_static(kind), line)
            .with_target(feed.target.clone());
        if let Some(channel) = &feed.channel {
            envelope = envelope.with_channel(channel.clone());
        }
        if let Some(session) = &feed.session {
            envelope = envelope.with_tag("session_id", session);
        }
        if let Some(update) = update {
            envelope = match update {
                FeedUpdate::Progress(value) => envelope.with_context(value),
                FeedUpdate::Text(value) => envelope.with_context(value),
            };
        }
        let mut envelope = binding.output(envelope);
        if matches!(kind, "albert.progress" | "albert.text") {
            // This is control-plane metadata. Keep the original SessionContext
            // as provenance, but never deliver an empty message to a chat adapter.
            envelope.target = Some(ConnectorId::new("web"));
        }
        if let Err(error) = feed.bus.publish(envelope).await {
            warn!(%error, "failed to publish live status");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use octo_core::{ConnectorId, EventBus, Filter, InProcessBus, SubscribeOptions};
    use rig::agent::PromptHook;
    use serde_json::to_string;
    use tokio::time::timeout;

    use super::ProgressUpdate;
    use crate::{
        codex_model::CodexResponsesModel, sessions::transport::TurnFeedback, status::StatusFeed,
    };

    #[test]
    fn goal_labels_show_only_the_requested_bounded_goal() {
        let feed = StatusFeed::new(
            Arc::new(InProcessBus::new(16)),
            ConnectorId::new("albert"),
            ConnectorId::new("web"),
            None,
        );
        assert_eq!(
            feed.progress_tool_label(
                "scratchpad_goal",
                r#"{"goal":"Compare\n  sources", "extra":"private-field"}"#
            ),
            "scratchpad goal “Compare sources”"
        );
        for args in ["invalid", r#"{"goal":17}"#, r#"{"goal":"  "}"#] {
            assert_eq!(
                feed.progress_tool_label("scratchpad_goal", args),
                "scratchpad goal"
            );
        }
        let label = feed.progress_tool_label(
            "scratchpad_goal",
            &format!(r#"{{"goal":"{}"}}"#, "a".repeat(200)),
        );
        assert!(label.chars().count() <= 160);
        assert!(label.ends_with("…”"));
        assert_eq!(
            feed.progress_tool_label("another_tool", r#"{"goal":"private-field"}"#),
            "another tool"
        );
    }

    #[tokio::test]
    async fn tool_projection_has_no_arguments_results_or_unregistered_names() {
        let bus = Arc::new(InProcessBus::new(16));
        let mut events = bus
            .subscribe(
                Filter::by_kind("albert.progress"),
                SubscribeOptions::default(),
            )
            .await
            .unwrap();
        let feed = StatusFeed::new(
            bus,
            ConnectorId::new("albert"),
            ConnectorId::new("web"),
            None,
        )
        .with_attempt("native-attempt".into());
        feed.register_tool_names(["safe_tool".into()]);
        <StatusFeed as PromptHook<CodexResponsesModel>>::on_tool_call(
            &feed,
            "safe_tool",
            None,
            "opaque-call",
            "private-fixture-content",
        )
        .await;
        <StatusFeed as PromptHook<CodexResponsesModel>>::on_tool_result(
            &feed,
            "safe_tool",
            None,
            "opaque-call",
            "private-fixture-content",
            "private-fixture-result",
        )
        .await;
        let start = timeout(Duration::from_secs(1), events.next())
            .await
            .unwrap()
            .unwrap();
        let end = timeout(Duration::from_secs(1), events.next())
            .await
            .unwrap()
            .unwrap();
        let start = &start.context.get::<ProgressUpdate>().unwrap().0;
        let end = &end.context.get::<ProgressUpdate>().unwrap().0;
        assert_eq!(start.attempt_id, "native-attempt");
        assert_eq!(start.items[0].id, end.items[0].id);
        assert_eq!(start.items[0].state, "in_progress");
        assert_eq!(end.items[0].state, "returned");
        assert!(end.items[0].elapsed_ms.is_some());
        let wire = to_string(&[start, end]).unwrap();
        assert!(!wire.contains("private-fixture"));
        <StatusFeed as PromptHook<CodexResponsesModel>>::on_tool_call(
            &feed,
            "private-unregistered-name",
            None,
            "other-call",
            "{}",
        )
        .await;
        let unknown = timeout(Duration::from_secs(1), events.next())
            .await
            .unwrap()
            .unwrap();
        assert!(
            !to_string(&unknown.context.get::<ProgressUpdate>().unwrap().0)
                .unwrap()
                .contains("private-unregistered-name")
        );
    }

    #[tokio::test]
    async fn closed_presentation_cannot_publish_late_progress() {
        let bus = Arc::new(InProcessBus::new(8));
        let mut events = bus
            .subscribe(
                Filter::by_kind("albert.progress"),
                SubscribeOptions::default(),
            )
            .await
            .unwrap();
        let feedback = TurnFeedback::new("attempt");
        let feed = StatusFeed::new(
            bus,
            ConnectorId::new("albert"),
            ConnectorId::new("web"),
            None,
        )
        .with_feedback(Some(feedback.clone()));
        feed.progress_phase("Thinking").await;
        assert!(timeout(Duration::from_secs(1), events.next())
            .await
            .unwrap()
            .is_some());
        feedback.close();
        feed.progress_phase("Late phase").await;
        assert!(timeout(Duration::from_millis(30), events.next())
            .await
            .is_err());
    }

    #[tokio::test]
    async fn pure_progress_targets_the_web_without_rewriting_legacy_status_delivery() {
        let bus = Arc::new(InProcessBus::new(8));
        let mut progress = bus
            .subscribe(
                Filter::by_kind("albert.progress"),
                SubscribeOptions::default(),
            )
            .await
            .unwrap();
        let mut console = bus
            .subscribe(
                Filter::by_target(ConnectorId::new("console")),
                SubscribeOptions::default(),
            )
            .await
            .unwrap();
        let feed = StatusFeed::new(
            bus,
            ConnectorId::new("albert"),
            ConnectorId::new("console"),
            None,
        )
        .with_attempt("native".into());
        feed.progress_phase("Thinking").await;
        let event = timeout(Duration::from_secs(1), progress.next())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(event.target.as_ref().unwrap().as_str(), "web");
        assert!(timeout(Duration::from_millis(20), console.next())
            .await
            .is_err());
        feed.progress("Compacting history").await;
        let legacy = timeout(Duration::from_secs(1), console.next())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(legacy.kind.as_str(), "chat.status");
        assert_eq!(legacy.payload_as::<String>().unwrap(), "Compacting history");
    }
}
