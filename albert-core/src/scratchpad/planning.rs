use std::collections::HashSet;

use serde_json::{json, Value};

use super::{PadScope, PlanArgs, Scratchpad, ScratchpadStore, Status};

impl ScratchpadStore {
    pub fn plan_action(&self, scope: &PadScope, args: PlanArgs) -> Value {
        if args.action == "get" || args.action == "get_stage" {
            let map = self.inner.lock().unwrap();
            let Some(pad) = map.get(scope) else {
                return json!({"ok":false,"error":"task not found"});
            };
            if args.task_id.as_ref().is_some_and(|id| id != &pad.task_id) {
                return json!({"ok":false,"error":"task not found"});
            }
            if args.action == "get_stage" {
                return self.bounded_page(match pad
                    .steps
                    .iter()
                    .find(|s| Some(&s.id) == args.stage_id.as_ref())
                {
                    Some(stage) => {
                        json!({"ok":true,"task_id":pad.task_id,"revision":pad.revision,"stage":stage})
                    }
                    None => json!({"ok":false,"error":"stage not found"}),
                });
            }
            let offset = args.offset.unwrap_or(0);
            let limit = args.limit.unwrap_or(self.limits.page_size);
            if limit == 0 || limit > self.limits.page_size {
                return json!({"ok":false,"error":"invalid page size"});
            }
            let stages:Vec<Value>=pad.steps.iter().skip(offset).take(limit).map(|s|json!({"id":s.id,"text":s.text,"status":s.status,"spec":s.spec,"note":s.note,"attempt_count":s.attempts.len(),"evidence_count":s.evidence.len()})).collect();
            return self.bounded_page(json!({"ok":true,"task_id":pad.task_id,"revision":pad.revision,"goal":pad.goal,"stages":stages,"total":pad.steps.len(),"offset":offset,"notes":pad.notes.iter().skip(offset).take(limit).collect::<Vec<_>>(),"notes_total":pad.notes.len()}));
        }
        self.mutate(scope, false, |pad| {
            Self::check_revision(pad, args.task_id.as_deref(), args.expected_revision)?;
            let stage_id = match args.action.as_str() {
                "add" => {
                    let text = args.text.ok_or("text required")?;
                    let spec = args.spec.ok_or("spec required")?;
                    let stage = self.stage(text, Some(spec));
                    let id = stage.id.clone();
                    pad.steps.push(stage);
                    Self::validate_graph(pad)?;
                    id
                }
                "update" | "mark" => {
                    let id = args.stage_id.ok_or("stage_id required")?;
                    let index = pad
                        .steps
                        .iter()
                        .position(|s| s.id == id)
                        .ok_or("stage not found")?;
                    if args.action == "update" {
                        if pad.steps[index].attempts.iter().any(|a| a.state.is_live()) {
                            return Err("cannot edit stage with live attempt".into());
                        }
                        let changed = args
                            .text
                            .as_ref()
                            .is_some_and(|t| t != &pad.steps[index].text)
                            || args
                                .spec
                                .as_ref()
                                .is_some_and(|s| Some(s) != pad.steps[index].spec.as_ref());
                        if changed {
                            let affected = Self::affected(pad, &id);
                            if pad
                                .steps
                                .iter()
                                .filter(|s| affected.contains(&s.id))
                                .any(|s| s.attempts.iter().any(|a| a.state.is_live()))
                            {
                                return Err("cannot edit stages with live attempts".into());
                            }
                            if let Some(text) = args.text {
                                pad.steps[index].text = text;
                            }
                            if let Some(spec) = args.spec {
                                pad.steps[index].spec = Some(spec);
                            }
                            Self::validate_graph(pad)?;
                            for stage in &mut pad.steps {
                                if affected.contains(&stage.id) {
                                    stage.evidence.clear();
                                    stage.status = Status::Pending;
                                }
                            }
                        }
                        if let Some(note) = args.note {
                            pad.steps[index].note = Some(note);
                        }
                    } else {
                        let status =
                            Status::parse(args.status.as_deref().ok_or("status required")?)
                                .ok_or("invalid status")?;
                        let stage = &pad.steps[index];
                        if stage.attempts.iter().any(|a| a.state.is_live()) {
                            return Err("cannot mark stage with live attempt".into());
                        }
                        if let Some(spec) = &stage.spec {
                            if matches!(status, Status::InProgress | Status::Verified) {
                                Self::ready(pad, &id)?;
                            }
                            let evidence = args.evidence.as_ref().unwrap_or(&stage.evidence);
                            if evidence.iter().any(|e| {
                                e.criterion >= spec.acceptance.len()
                                    || e.reference.trim().is_empty()
                                    || e.summary.trim().is_empty()
                            }) {
                                return Err("invalid acceptance evidence".into());
                            }
                            if status == Status::Verified
                                && (0..spec.acceptance.len())
                                    .any(|i| !evidence.iter().any(|e| e.criterion == i))
                            {
                                return Err(
                                    "evidence required for every acceptance criterion".into()
                                );
                            }
                        }
                        if status != Status::Verified && stage.status == Status::Verified {
                            let affected = Self::affected(pad, &id);
                            if pad
                                .steps
                                .iter()
                                .filter(|s| s.id != id && affected.contains(&s.id))
                                .any(|s| s.attempts.iter().any(|a| a.state.is_live()))
                            {
                                return Err("dependent stage has live attempt".into());
                            }
                            for stage in &mut pad.steps {
                                if stage.id != id && affected.contains(&stage.id) {
                                    stage.status = Status::Pending;
                                    stage.evidence.clear();
                                }
                            }
                        }
                        let stage = &mut pad.steps[index];
                        stage.status = status;
                        if let Some(note) = args.note {
                            stage.note = Some(note);
                        }
                        if let Some(evidence) = args.evidence {
                            stage.evidence = evidence;
                        }
                    }
                    id
                }
                _ => return Err("action must be get|get_stage|add|update|mark".into()),
            };
            Ok(json!({"ok":true,"stage_id":stage_id}))
        })
    }
    pub(super) fn check_revision(
        pad: &Scratchpad,
        task: Option<&str>,
        revision: Option<u64>,
    ) -> Result<(), String> {
        if task != Some(pad.task_id.as_str()) {
            return Err("task_id mismatch or missing".into());
        }
        if revision != Some(pad.revision) {
            return Err("stale or missing expected_revision".into());
        }
        Ok(())
    }
    pub(super) fn affected(pad: &Scratchpad, id: &str) -> HashSet<String> {
        let mut ids = HashSet::from([id.to_owned()]);
        loop {
            let old = ids.len();
            for stage in &pad.steps {
                if stage
                    .spec
                    .as_ref()
                    .is_some_and(|s| s.depends_on.iter().any(|d| ids.contains(d)))
                {
                    ids.insert(stage.id.clone());
                }
            }
            if old == ids.len() {
                return ids;
            }
        }
    }
    fn validate_graph(pad: &Scratchpad) -> Result<(), String> {
        for stage in &pad.steps {
            if let Some(spec) = &stage.spec {
                for dependency in &spec.depends_on {
                    if dependency == &stage.id || !pad.steps.iter().any(|s| &s.id == dependency) {
                        return Err("self or dangling dependency".into());
                    }
                    if Self::affected(pad, &stage.id).contains(dependency) {
                        return Err("dependency cycle".into());
                    }
                }
            }
        }
        Ok(())
    }
    pub(super) fn ready(pad: &Scratchpad, id: &str) -> Result<(), String> {
        let stage = pad
            .steps
            .iter()
            .find(|s| s.id == id)
            .ok_or("stage not found")?;
        let spec = stage.spec.as_ref().ok_or("planned stage required")?;
        if spec.depends_on.iter().any(|id| {
            !pad.steps
                .iter()
                .any(|s| &s.id == id && s.status == Status::Verified)
        }) {
            return Err("dependencies must be verified".into());
        }
        Ok(())
    }
}
