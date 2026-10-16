use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};

use rand::random;
use serde_json::{from_str, json, to_string, Value};

use super::{Limits, PadHandle, PadScope, Scratchpad, ScratchpadStore, Stage, StageSpec, Status};

impl ScratchpadStore {
    #[cfg(test)]
    pub fn new() -> Arc<Self> {
        Self::with_limits(Limits::default()).expect("valid default limits")
    }
    pub fn with_limits(limits: Limits) -> Result<Arc<Self>, String> {
        limits.validate()?;
        Ok(Arc::new(Self {
            inner: Mutex::new(HashMap::new()),
            limits,
            sequence: AtomicU64::new(1),
            instance: random(),
        }))
    }
    pub(super) fn id(&self, prefix: &str) -> String {
        format!(
            "{prefix}-{:016x}-{}",
            self.instance,
            self.sequence.fetch_add(1, Ordering::Relaxed)
        )
    }
    pub(super) fn legacy_scope(channel: &str) -> PadScope {
        PadScope {
            conversation: channel.into(),
            owner: false,
        }
    }
    pub fn handle(self: &Arc<Self>, channel: impl Into<String>) -> PadHandle {
        self.scoped_handle(Self::legacy_scope(&channel.into()))
    }
    pub fn scoped_handle(self: &Arc<Self>, scope: PadScope) -> PadHandle {
        PadHandle::new(Arc::clone(self), scope)
    }
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
    fn truncate_render(mut text: String, limit: usize) -> String {
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

    pub(super) fn fresh(&self) -> Scratchpad {
        Scratchpad {
            task_id: self.id("task"),
            revision: 0,
            goal: None,
            steps: Vec::new(),
            notes: Vec::new(),
        }
    }
    pub(super) fn validate(&self, pad: &Scratchpad) -> Result<(), String> {
        if pad.steps.len() > self.limits.stages || pad.notes.len() > self.limits.notes {
            return Err("scratchpad count limit exceeded".into());
        }
        for stage in &pad.steps {
            if stage.attempts.len() > self.limits.attempts {
                return Err("stage attempt limit exceeded".into());
            }
        }
        let value = to_string(pad).map_err(|e| e.to_string())?;
        if value.len() > self.limits.pad_bytes {
            return Err("scratchpad byte limit exceeded".into());
        }
        // Every string, including nested assignment/evidence and attempt fields, is bounded.
        let value: Value = from_str(&value).map_err(|e| e.to_string())?;
        self.validate_value(&value)
    }
    pub(super) fn bounded_page(&self, value: Value) -> Value {
        match to_string(&value) {
            Ok(serialized) if serialized.len() <= self.limits.page_bytes => value,
            _ => {
                json!({"ok":false,"error":"scratchpad page byte limit exceeded; request a smaller page"})
            }
        }
    }
    pub(super) fn validate_value(&self, value: &Value) -> Result<(), String> {
        match value {
            Value::String(s) if s.len() > self.limits.text_bytes => {
                Err("scratchpad text limit exceeded".into())
            }
            Value::Array(a) => {
                if a.len()
                    > self
                        .limits
                        .stages
                        .max(self.limits.notes)
                        .max(self.limits.attempts)
                {
                    return Err("nested list limit exceeded".into());
                }
                for v in a {
                    self.validate_value(v)?;
                }
                Ok(())
            }
            Value::Object(o) => {
                for v in o.values() {
                    self.validate_value(v)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
    pub(super) fn mutate<F>(&self, scope: &PadScope, create: bool, f: F) -> Value
    where
        F: FnOnce(&mut Scratchpad) -> Result<Value, String>,
    {
        let mut map = self.inner.lock().unwrap();
        if !map.contains_key(scope) && (!create || map.len() >= self.limits.pads) {
            return json!({"ok":false,"error":"scratchpad missing or pad limit exceeded"});
        }
        let mut pad = map.get(scope).cloned().unwrap_or_else(|| self.fresh());
        match f(&mut pad).and_then(|value| {
            self.validate(&pad)?;
            Ok(value)
        }) {
            Ok(mut value) => {
                pad.revision += 1;
                value["task_id"] = json!(pad.task_id);
                value["revision"] = json!(pad.revision);
                map.insert(scope.clone(), pad);
                value
            }
            Err(error) => {
                json!({"ok":false,"error":error,"task_id":map.get(scope).map(|p|&p.task_id),"revision":map.get(scope).map(|p|p.revision)})
            }
        }
    }
    pub(super) fn live_runs(pad: &Scratchpad) -> Vec<String> {
        pad.steps
            .iter()
            .flat_map(|s| &s.attempts)
            .filter(|a| a.state.is_live())
            .filter_map(|a| a.run_id.clone())
            .collect()
    }
    pub(super) fn set_goal(&self, scope: &PadScope, goal: String) -> Value {
        self.mutate(scope, true, |pad| {
            let runs = Self::live_runs(pad);
            let revision = pad.revision;
            *pad = self.fresh();
            pad.revision = revision;
            pad.goal = Some(goal.clone());
            Ok(json!({"ok":true,"goal":goal,"live_run_ids":runs}))
        })
    }
    pub(super) fn add_step(&self, scope: &PadScope, text: String) -> Value {
        self.mutate(scope, true, |pad| {
            pad.steps.push(self.stage(text, None));
            Ok(json!({"ok":true,"step":pad.steps.len()}))
        })
    }
    pub(super) fn stage(&self, text: String, spec: Option<StageSpec>) -> Stage {
        Stage {
            id: self.id("stage"),
            text,
            status: Status::Pending,
            note: None,
            spec,
            attempts: Vec::new(),
            evidence: Vec::new(),
        }
    }
    pub(super) fn mark(
        &self,
        scope: &PadScope,
        step: usize,
        status: &str,
        note: Option<String>,
    ) -> Value {
        self.mutate(scope, false, |pad| {
            let parsed = Status::parse(status).ok_or("invalid status")?;
            let item = step
                .checked_sub(1)
                .and_then(|i| pad.steps.get_mut(i))
                .ok_or("no such step")?;
            if item.spec.is_some() {
                return Err("planned stages require scratchpad_plan".into());
            }
            item.status = parsed;
            if note.is_some() {
                item.note = note;
            }
            Ok(json!({"ok":true,"step":step,"status":parsed.as_str()}))
        })
    }
    pub(super) fn note(&self, scope: &PadScope, text: String) -> Value {
        self.mutate(scope, true, |pad| {
            pad.notes.push(text);
            Ok(json!({"ok":true}))
        })
    }
    pub(super) fn clear(&self, scope: &PadScope) -> Value {
        let old = self.inner.lock().unwrap().remove(scope);
        json!({"ok":true,"cleared":old.is_some(),"task_id":old.as_ref().map(|p|&p.task_id),"revision":old.as_ref().map(|p|p.revision+1),"live_run_ids":old.as_ref().map(Self::live_runs).unwrap_or_default()})
    }
    pub fn get_stage(
        &self,
        scope: &PadScope,
        task_id: &str,
        stage_id: &str,
    ) -> Result<Stage, String> {
        let map = self.inner.lock().unwrap();
        let pad = map
            .get(scope)
            .filter(|p| p.task_id == task_id)
            .ok_or("task not found")?;
        pad.steps
            .iter()
            .find(|s| s.id == stage_id)
            .cloned()
            .ok_or_else(|| "stage not found".into())
    }
}
