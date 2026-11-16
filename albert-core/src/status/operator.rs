//! Sanitized observation independent of the selected chat's presentation lease.
use octo_core::{ConnectorId, Envelope, EventBus, EventKind};

use super::{
    domain::ProgressActivity, domain::ProgressDelta, progress::ProgressUpdate, StatusFeed,
};

pub fn activity_label(activity: Option<ProgressActivity>, kind: &str) -> &'static str {
    match activity {
        Some(ProgressActivity::Scratchpad) => "Planning the task",
        Some(ProgressActivity::Memory) => "Working with memory",
        Some(ProgressActivity::Skill) => "Using a skill",
        Some(ProgressActivity::Search) => "Searching the web",
        Some(ProgressActivity::Browser) => "Reading a web page",
        Some(ProgressActivity::Files) => "Working with files",
        Some(ProgressActivity::Shell) => "Running a command",
        Some(ProgressActivity::Sql) => "Working with a database",
        Some(ProgressActivity::Calendar) => "Working with the calendar",
        Some(ProgressActivity::Mail) => "Working with mail",
        Some(ProgressActivity::Subagent) => "Subagent activity",
        Some(ProgressActivity::Sessions) => "Working with sessions",
        Some(ProgressActivity::Configuration) => "Updating configuration",
        Some(ProgressActivity::Model) => "Working with a model",
        Some(ProgressActivity::Image) => "Working with an image",
        Some(ProgressActivity::AudioInput) => "Transcribing audio",
        Some(ProgressActivity::AudioOutput) => "Generating speech",
        Some(ProgressActivity::Connector) => "Using a connection",
        _ => match kind {
            "stage" => "Task step",
            "child" => "Subagent activity",
            "tool" => "Using a tool",
            _ => "Thinking",
        },
    }
}
impl StatusFeed {
    pub(super) async fn operator_progress(&self, delta: &ProgressDelta) {
        let Some(session) = self
            .session_context
            .as_ref()
            .filter(|s| s.is_private() && s.is_live())
        else {
            return;
        };
        if !self.is_live() {
            return;
        }
        let Some(feed) = &self.feed else { return };
        let mut delta = delta.clone();
        for item in &mut delta.items {
            item.label =
                if item.kind == "phase" && item.label.to_ascii_lowercase().contains("compact") {
                    "Compacting context"
                } else {
                    activity_label(item.activity, item.kind.as_str())
                }
                .into();
        }
        let envelope = Envelope::new(
            feed.source.clone(),
            EventKind::new("albert.operator.progress"),
            (),
        )
        .with_target(ConnectorId::new("web"))
        .with_context(session.clone())
        .with_context(ProgressUpdate(delta));
        let _ = feed.bus.publish(envelope).await;
    }
}
