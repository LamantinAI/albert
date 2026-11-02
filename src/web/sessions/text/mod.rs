use std::collections::VecDeque;

use crate::{
    api::models::{TextDelta, TextSnapshot},
    status::text::MAX_TEXT_BYTES,
};

#[derive(Default)]
pub(super) struct TextPreview {
    current: Option<TextSnapshot>,
    retired: VecDeque<String>,
}
impl TextPreview {
    pub fn apply(&mut self, delta: &TextDelta) -> bool {
        if delta.attempt_id.is_empty()
            || delta.attempt_id.len() > 256
            || delta.generation.is_empty()
            || delta.generation.len() > 256
            || delta.text.len() > 4096
        {
            return false;
        }
        let identity = format!("{}/{}", delta.attempt_id, delta.generation);
        if delta.operation == "start" {
            if self.current.as_ref().is_some_and(|s| {
                s.attempt_id == delta.attempt_id && s.generation == delta.generation
            }) || self.retired.contains(&identity)
            {
                return false;
            }
            if let Some(old) = self.current.take() {
                self.retired
                    .push_back(format!("{}/{}", old.attempt_id, old.generation));
                if self.retired.len() > 32 {
                    self.retired.pop_front();
                }
            }
            self.current = Some(TextSnapshot {
                attempt_id: delta.attempt_id.clone(),
                generation: delta.generation.clone(),
                text: String::new(),
                offset: 0,
                state: "streaming".into(),
                truncated: false,
                message_ids: vec![],
            });
            return true;
        }
        let Some(current) = self
            .current
            .as_mut()
            .filter(|s| s.attempt_id == delta.attempt_id && s.generation == delta.generation)
        else {
            return false;
        };
        if current.state == "committed" {
            return false;
        }
        match delta.operation.as_str() {
            "append"
                if delta.offset == current.offset
                    && !current.truncated
                    && current.state == "streaming" =>
            {
                if current.text.len() + delta.text.len() > MAX_TEXT_BYTES {
                    current.truncated = true;
                    return true;
                }
                current.text.push_str(&delta.text);
                current.offset += delta.text.len();
            }
            "truncated" => current.truncated = true,
            "reset" => {
                current.text.clear();
                current.offset = 0;
                current.state = "waiting".into();
            }
            "committed" => {
                current.text.clear();
                current.state = "committed".into();
                current.message_ids = delta.message_ids.iter().take(16).cloned().collect();
            }
            _ => return false,
        }
        true
    }
    pub fn snapshot(&self, active: Option<&str>) -> Option<TextSnapshot> {
        let mut snapshot = self.current.clone()?;
        if active.is_some_and(|id| id != snapshot.attempt_id) {
            return None;
        }
        if active.is_none() && snapshot.state == "streaming" {
            snapshot.state = "interrupted".into();
        }
        Some(snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::TextPreview;
    use crate::api::models::TextDelta;
    #[test]
    fn offsets_generations_and_commit_fence_preview() {
        let mut state = TextPreview::default();
        let mut event = TextDelta {
            attempt_id: "one".into(),
            generation: "first".into(),
            operation: "start".into(),
            offset: 0,
            text: String::new(),
            message_ids: vec![],
        };
        assert!(state.apply(&event));
        event.operation = "append".into();
        event.text = "Привет".into();
        assert!(state.apply(&event));
        assert!(!state.apply(&event));
        event.operation = "start".into();
        event.text.clear();
        event.generation = "next".into();
        assert!(state.apply(&event));
        event.generation = "first".into();
        assert!(!state.apply(&event));
        event.generation = "next".into();
        event.operation = "committed".into();
        event.message_ids = vec!["reply".into()];
        assert!(state.apply(&event));
        event.operation = "append".into();
        assert!(!state.apply(&event));
        assert_eq!(state.snapshot(None).unwrap().message_ids, ["reply"]);
    }
}
