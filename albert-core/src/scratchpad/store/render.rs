use crate::scratchpad::{PadScope, ScratchpadStore};
use serde_json::json;

impl ScratchpadStore {
    #[cfg(test)]
    pub fn render(&self, channel: &str) -> String {
        self.render_scoped(&Self::legacy_scope(channel))
    }
    pub fn render_scoped(&self, scope: &PadScope) -> String {
        let map = self.inner.lock().unwrap();
        let Some(pad) = map.get(scope) else {
            return Self::truncate_render(
                "Scratchpad: empty (no active multi-step task).".into(),
                self.limits.render_bytes,
            );
        };
        let mut s = format!(
            "Scratchpad task={} revision={}\nGoal: {}\nSteps:\n",
            pad.task_id,
            pad.revision,
            pad.goal.as_deref().unwrap_or("(none set)")
        );
        for (i, stage) in pad.steps.iter().enumerate() {
            s += &format!(
                "  {}. [{}] {} ({})",
                i + 1,
                stage.status.as_str(),
                stage.text,
                stage.id
            );
            if let Some(note) = &stage.note {
                s += &format!(" — {note}");
            }
            if let Some(spec) = &stage.spec {
                let assignment = &spec.assignment;
                s += &format!(
                    " | role={} models={} effort={}",
                    assignment.role.as_deref().unwrap_or("(none)"),
                    if assignment.models.is_empty() {
                        "(role/primary default)".into()
                    } else {
                        assignment.models.join(",")
                    },
                    assignment
                        .reasoning_effort
                        .as_deref()
                        .unwrap_or("(default)"),
                );
                if let Some(attempt) = stage.attempts.last() {
                    s += &format!(
                        " | run={} state={}",
                        attempt.run_id.as_deref().unwrap_or("(unbound)"),
                        json!(attempt.state)
                    );
                }
            }
            s += "\n";
        }
        for note in &pad.notes {
            s += &format!("  - {note}\n");
        }
        Self::truncate_render(s, self.limits.render_bytes)
    }
    pub(in crate::scratchpad) fn truncate_render(mut text: String, limit: usize) -> String {
        if text.len() <= limit {
            return text;
        }
        let marker = "\n[truncated; use scratchpad_plan]";
        let marker = &marker[..marker.len().min(limit)];
        let mut end = limit - marker.len();
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str(marker);
        text
    }
}
