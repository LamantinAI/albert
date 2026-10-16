use super::{
    Assignment, AttemptState, PadScope, Reservation, ResolvedAssignment, ScratchpadStore,
    StageAttempt, StageLink, Status,
};

impl ScratchpadStore {
    pub fn reserve_stage(
        &self,
        scope: &PadScope,
        link: &StageLink,
        resolved: ResolvedAssignment,
    ) -> Result<Reservation, String> {
        let mut map = self.inner.lock().unwrap();
        let original = map
            .get(scope)
            .filter(|p| p.task_id == link.task_id)
            .ok_or("task not found")?;
        let stage = original
            .steps
            .iter()
            .find(|s| s.id == link.stage_id)
            .ok_or("stage not found")?;
        if let Some(attempt) = stage
            .attempts
            .iter()
            .find(|a| a.launch_key == link.launch_key)
        {
            if attempt.requested_revision != link.expected_revision
                || attempt.assignment != resolved
            {
                return Err("launch key reused with different request".into());
            }
            return Ok(Reservation::Existing {
                attempt_id: attempt.id.clone(),
                run_id: attempt.run_id.clone(),
                state: attempt.state,
            });
        }
        Self::check_revision(original, Some(&link.task_id), Some(link.expected_revision))?;
        if link.launch_key.trim().is_empty() || resolved.models.is_empty() {
            return Err("launch key and resolved models required".into());
        }
        Self::ready(original, &link.stage_id)?;
        if stage.attempts.iter().any(|a| a.state.is_live()) {
            return Err("stage has live attempt".into());
        }
        let affected = Self::affected(original, &link.stage_id);
        if original
            .steps
            .iter()
            .filter(|s| s.id != link.stage_id && affected.contains(&s.id))
            .any(|s| s.attempts.iter().any(|a| a.state.is_live()))
        {
            return Err("dependent stage has live attempt".into());
        }
        let mut pad = original.clone();
        for stage in &mut pad.steps {
            if affected.contains(&stage.id) {
                stage.status = Status::Pending;
                stage.evidence.clear();
            }
        }
        let stage = pad
            .steps
            .iter_mut()
            .find(|s| s.id == link.stage_id)
            .ok_or("stage not found")?;
        let attempt_id = self.id("attempt");
        stage.attempts.push(StageAttempt {
            id: attempt_id.clone(),
            launch_key: link.launch_key.clone(),
            requested_revision: link.expected_revision,
            assignment: resolved,
            run_id: None,
            state: AttemptState::Reserved,
            actual_models: Vec::new(),
            outcome_summary: None,
        });
        stage.status = Status::InProgress;
        self.validate(&pad)?;
        pad.revision += 1;
        map.insert(scope.clone(), pad);
        Ok(Reservation::New { attempt_id })
    }
    fn attempt_mutation<F>(
        &self,
        scope: &PadScope,
        task_id: &str,
        stage_id: &str,
        attempt_id: &str,
        f: F,
    ) -> Result<(), String>
    where
        F: FnOnce(&mut StageAttempt) -> Result<(), String>,
    {
        let mut map = self.inner.lock().unwrap();
        let mut pad = map
            .get(scope)
            .filter(|p| p.task_id == task_id)
            .cloned()
            .ok_or("task not found")?;
        let stage = pad
            .steps
            .iter_mut()
            .find(|s| s.id == stage_id)
            .ok_or("stage not found")?;
        let attempt = stage
            .attempts
            .iter_mut()
            .find(|a| a.id == attempt_id)
            .ok_or("attempt not found")?;
        f(attempt)?;
        self.validate(&pad)?;
        pad.revision += 1;
        map.insert(scope.clone(), pad);
        Ok(())
    }
    pub fn bind_run(
        &self,
        scope: &PadScope,
        task_id: &str,
        stage_id: &str,
        attempt_id: &str,
        run_id: &str,
    ) -> Result<(), String> {
        if run_id.trim().is_empty() {
            return Err("run_id required".into());
        }
        self.attempt_mutation(scope, task_id, stage_id, attempt_id, |a| {
            if a.state == AttemptState::Running && a.run_id.as_deref() == Some(run_id) {
                return Ok(());
            }
            if a.state != AttemptState::Reserved || a.run_id.is_some() {
                return Err("attempt already bound or terminal".into());
            }
            a.run_id = Some(run_id.into());
            a.state = AttemptState::Running;
            Ok(())
        })
    }
    pub fn finish_attempt(
        &self,
        scope: &PadScope,
        task_id: &str,
        stage_id: &str,
        attempt_id: &str,
        state: AttemptState,
        outcome_summary: Option<String>,
    ) -> Result<(), String> {
        if state.is_live() {
            return Err("terminal state required".into());
        }
        self.attempt_mutation(scope, task_id, stage_id, attempt_id, |a| {
            if !a.state.is_live() {
                return Err("attempt already terminal".into());
            }
            a.state = state;
            a.outcome_summary = outcome_summary;
            Ok(())
        })
    }
    pub fn release_reservation(
        &self,
        scope: &PadScope,
        task_id: &str,
        stage_id: &str,
        attempt_id: &str,
    ) -> Result<(), String> {
        self.attempt_mutation(scope, task_id, stage_id, attempt_id, |a| {
            if a.state != AttemptState::Reserved || a.run_id.is_some() {
                return Err("only unbound reservation can be released".into());
            }
            a.state = AttemptState::Failed;
            a.outcome_summary = Some("launch failed before run binding".into());
            Ok(())
        })
    }
    pub fn append_actual_model(
        &self,
        scope: &PadScope,
        task_id: &str,
        stage_id: &str,
        attempt_id: &str,
        model: String,
    ) -> Result<(), String> {
        if model.trim().is_empty() {
            return Err("actual model required".into());
        }
        self.attempt_mutation(scope, task_id, stage_id, attempt_id, |a| {
            if !a.state.is_live() {
                return Err("attempt already terminal".into());
            }
            a.actual_models.push(model);
            Ok(())
        })
    }
    pub fn stage_assignment(
        &self,
        scope: &PadScope,
        task_id: &str,
        stage_id: &str,
    ) -> Result<Assignment, String> {
        self.get_stage(scope, task_id, stage_id)?
            .spec
            .map(|s| s.assignment)
            .ok_or_else(|| "planned stage required".into())
    }
}
