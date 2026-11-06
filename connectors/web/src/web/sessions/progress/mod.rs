//! Bounded, attempt-scoped presentation state. Session admission remains the
//! authority on whether execution is active; a presentation ending is not that.
use std::collections::VecDeque;

use crate::{api::models::ProgressDelta, api::models::ProgressSnapshot};

const MAX_ITEMS: usize = 32;

#[derive(Default)]
pub(super) struct Progress {
    snapshot: Option<ProgressSnapshot>,
    retired: VecDeque<String>,
}

impl Progress {
    pub fn apply(&mut self, delta: &ProgressDelta) -> bool {
        if delta.attempt_id.is_empty()
            || delta.attempt_id.len() > 256
            || self.retired.contains(&delta.attempt_id)
        {
            return false;
        }
        if self
            .snapshot
            .as_ref()
            .is_none_or(|snapshot| snapshot.attempt_id != delta.attempt_id)
        {
            if let Some(old) = self.snapshot.take() {
                self.retired.push_back(old.attempt_id);
                if self.retired.len() > 8 {
                    self.retired.pop_front();
                }
            }
            self.snapshot = Some(ProgressSnapshot {
                attempt_id: delta.attempt_id.clone(),
                started_at: delta.at.clone(),
                updated_at: delta.at.clone(),
                items: Vec::new(),
                truncated: false,
            });
        }
        let snapshot = self.snapshot.as_mut().expect("initialized progress");
        snapshot.updated_at = delta.at.clone();
        if delta.replace_kind.as_deref() == Some("stage") {
            snapshot.items.retain(|item| item.kind != "stage");
        }
        for mut item in delta.items.iter().take(MAX_ITEMS).cloned() {
            if item.id.len() > 256
                || !["phase", "tool", "stage", "child"].contains(&item.kind.as_str())
            {
                continue;
            }
            item.label = item
                .label
                .chars()
                .filter(|c| !c.is_control())
                .take(160)
                .collect();
            if (item.kind == "phase" || item.kind == "tool") && item.state == "in_progress" {
                for previous in &mut snapshot.items {
                    if previous.kind == "phase" && previous.state == "in_progress" {
                        previous.state = "returned".into();
                        previous.updated_at = delta.at.clone();
                    }
                }
            }
            if let Some(previous) = snapshot
                .items
                .iter_mut()
                .find(|previous| previous.id == item.id)
            {
                // A fast child may finish before the spawn caller publishes its
                // initial running observation. Completion never moves backwards.
                if previous.kind == "child"
                    && previous.state != "running"
                    && item.state == "running"
                {
                    continue;
                }
                item.started_at = previous.started_at.clone();
                *previous = item;
            } else {
                snapshot.items.push(item);
            }
            if snapshot.items.len() > MAX_ITEMS {
                let drop = snapshot
                    .items
                    .iter()
                    .position(|item| {
                        !["in_progress", "running", "waiting"].contains(&item.state.as_str())
                    })
                    .unwrap_or(0);
                snapshot.items.remove(drop);
                snapshot.truncated = true;
            }
        }
        true
    }

    pub fn snapshot(&self, active: Option<&str>) -> Option<ProgressSnapshot> {
        let mut snapshot = self.snapshot.clone()?;
        if active.is_some_and(|id| id != snapshot.attempt_id) {
            return None;
        }
        if active.is_none() {
            for item in &mut snapshot.items {
                if ["in_progress", "running", "waiting"].contains(&item.state.as_str())
                    && item.kind != "stage"
                {
                    item.state = if item.kind == "phase" {
                        "returned"
                    } else {
                        "unknown"
                    }
                    .into();
                }
            }
        }
        Some(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::Progress;
    use crate::{
        api::models::ProgressActivity, api::models::ProgressDelta, api::models::ProgressItem,
    };

    #[test]
    fn typed_activity_survives_completion_and_snapshot_reconciliation() {
        let mut progress = Progress::default();
        let mut start = change("work", "memory", "tool", "in_progress");
        start.items[0].activity = Some(ProgressActivity::Memory);
        assert!(progress.apply(&start));
        assert_eq!(
            progress.snapshot(Some("work")).unwrap().items[0].activity,
            Some(ProgressActivity::Memory)
        );
        let mut end = start;
        end.items[0].state = "returned".into();
        assert!(progress.apply(&end));
        assert_eq!(
            progress.snapshot(None).unwrap().items[0].activity,
            Some(ProgressActivity::Memory)
        );
    }

    fn change(attempt: &str, id: &str, kind: &str, state: &str) -> ProgressDelta {
        ProgressDelta {
            attempt_id: attempt.into(),
            at: "2030-01-01T00:00:00Z".into(),
            replace_kind: None,
            items: vec![ProgressItem {
                id: id.into(),
                kind: kind.into(),
                label: "Operation".into(),
                state: state.into(),
                started_at: "2030-01-01T00:00:00Z".into(),
                updated_at: "2030-01-01T00:00:00Z".into(),
                elapsed_ms: None,
                activity: None,
            }],
        }
    }

    #[test]
    fn superseded_attempt_cannot_reopen_or_overwrite_current_progress() {
        let mut progress = Progress::default();
        assert!(progress.apply(&change("first", "one", "tool", "in_progress")));
        assert!(progress.apply(&change("second", "two", "phase", "in_progress")));
        assert!(!progress.apply(&change("first", "one", "tool", "returned")));
        assert!(progress.snapshot(Some("first")).is_none());
        let current = progress.snapshot(Some("second")).unwrap();
        assert_eq!(current.items.len(), 1);
        assert_eq!(current.items[0].id, "two");
        assert_eq!(progress.snapshot(None).unwrap().items[0].state, "returned");
    }

    #[test]
    fn completion_is_monotonic_and_stage_done_is_not_verified() {
        let mut progress = Progress::default();
        progress.apply(&change("work", "child", "child", "completed"));
        progress.apply(&change("work", "child", "child", "running"));
        progress.apply(&change("work", "stage", "stage", "done"));
        let snapshot = progress.snapshot(Some("work")).unwrap();
        assert_eq!(snapshot.items[0].state, "completed");
        assert_eq!(snapshot.items[1].state, "done");
        progress.apply(&change("work", "stage", "stage", "verified"));
        assert_eq!(
            progress.snapshot(Some("work")).unwrap().items[1].state,
            "verified"
        );
    }

    #[test]
    fn unfinished_tools_are_unknown_after_execution_ends_and_history_is_bounded() {
        let mut progress = Progress::default();
        for n in 0..100 {
            progress.apply(&change("work", &n.to_string(), "tool", "returned"));
        }
        progress.apply(&change("work", "pending", "tool", "in_progress"));
        let live = progress.snapshot(Some("work")).unwrap();
        assert!(live.truncated);
        assert_eq!(live.items.len(), 32);
        assert_eq!(live.items.last().unwrap().state, "in_progress");
        assert_eq!(
            progress.snapshot(None).unwrap().items.last().unwrap().state,
            "unknown"
        );
    }
}
