//! Bounded RAM scratchpads, isolated by opaque conversation and owner scope.
mod attempts;
mod domain;
mod planning;
mod store;
mod tools;

use std::{collections::HashMap, sync::atomic::AtomicU64, sync::Mutex};

#[cfg(test)]
use domain::Evidence;
pub use domain::{
    Assignment, AttemptState, Limits, PadScope, PlanArgs, Reservation, ResolvedAssignment,
    Scratchpad, Settings, Stage, StageAttempt, StageLink, StageSpec, Status,
};
pub use tools::PadHandle;

pub struct ScratchpadStore {
    pub(super) inner: Mutex<HashMap<PadScope, Scratchpad>>,
    pub(super) limits: Limits,
    pub(super) sequence: AtomicU64,
    pub(super) instance: u64,
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::{from_value, json, Value};

    use super::{
        AttemptState, Evidence, Limits, PadScope, PlanArgs, Reservation, ResolvedAssignment,
        ScratchpadStore, StageLink, Status,
    };

    fn scope() -> PadScope {
        PadScope {
            conversation: "chat:opaque".into(),
            owner: true,
        }
    }
    fn action(store: &ScratchpadStore, scope: &PadScope, value: Value) -> Value {
        store.plan_action(scope, from_value::<PlanArgs>(value).unwrap())
    }
    fn state(store: &ScratchpadStore, scope: &PadScope) -> Value {
        action(store, scope, json!({"action":"get"}))
    }
    fn add(store: &ScratchpadStore, scope: &PadScope, text: &str, deps: Vec<String>) -> String {
        let pad = state(store, scope);
        let result = action(
            store,
            scope,
            json!({"action":"add","task_id":pad["task_id"],"expected_revision":pad["revision"],"text":text,"spec":{"depends_on":deps,"acceptance":["checked"]}}),
        );
        assert_eq!(result["ok"], true, "{result}");
        result["stage_id"].as_str().unwrap().into()
    }
    fn mark(
        store: &ScratchpadStore,
        scope: &PadScope,
        id: &str,
        status: &str,
        evidence: bool,
    ) -> Value {
        let pad = state(store, scope);
        let mut value = json!({"action":"mark","task_id":pad["task_id"],"expected_revision":pad["revision"],"stage_id":id,"status":status});
        if evidence {
            value["evidence"] =
                json!([{"criterion":0,"reference":"test:pass","summary":"assertions passed"}]);
        }
        action(store, scope, value)
    }
    fn prepared() -> (
        Arc<ScratchpadStore>,
        PadScope,
        String,
        StageLink,
        ResolvedAssignment,
    ) {
        let store = ScratchpadStore::new();
        let scope = scope();
        store.set_goal(&scope, "ship".into());
        let id = add(&store, &scope, "implement", vec![]);
        let pad = state(&store, &scope);
        let link = StageLink {
            task_id: pad["task_id"].as_str().unwrap().into(),
            stage_id: id.clone(),
            expected_revision: pad["revision"].as_u64().unwrap(),
            launch_key: "launch-1".into(),
        };
        let assignment = ResolvedAssignment {
            role: None,
            models: vec!["primary".into()],
            reasoning_effort: None,
        };
        (store, scope, id, link, assignment)
    }
    #[test]
    fn legacy_compatibility_and_scope_isolation() {
        let store = ScratchpadStore::new();
        let scope = ScratchpadStore::legacy_scope("stdin");
        assert!(store.render("stdin").contains("empty"));
        assert_eq!(store.set_goal(&scope, "ship the thing".into())["ok"], true);
        assert_eq!(store.add_step(&scope, "write it".into())["step"], 1);
        assert_eq!(store.add_step(&scope, "test it".into())["step"], 2);
        assert_eq!(store.mark(&scope, 1, "nonsense", None)["ok"], false);
        assert_eq!(store.mark(&scope, 9, "done", None)["ok"], false);
        assert_eq!(store.mark(&scope, 1, "verified", None)["ok"], true);
        store.note(&scope, "found the bug".into());
        let view = store.render("stdin");
        assert!(view.contains("[verified] write it"));
        assert!(view.contains("found the bug"));
        assert!(store
            .render_scoped(&PadScope {
                conversation: "stdin".into(),
                owner: true
            })
            .contains("empty"));
        assert_eq!(store.clear(&scope)["cleared"], true);
        assert!(store.render("stdin").contains("empty"));
    }
    #[test]
    fn cas_cycles_readiness_and_evidence() {
        let store = ScratchpadStore::new();
        let scope = scope();
        store.set_goal(&scope, "ship".into());
        let before = state(&store, &scope);
        let first = add(&store, &scope, "first", vec![]);
        let second = add(&store, &scope, "second", vec![first.clone()]);
        let stale = action(
            &store,
            &scope,
            json!({"action":"add","task_id":before["task_id"],"expected_revision":before["revision"],"text":"stale","spec":{}}),
        );
        assert_eq!(stale["ok"], false);
        assert_eq!(
            mark(&store, &scope, &second, "in_progress", false)["ok"],
            false
        );
        assert_eq!(mark(&store, &scope, &first, "verified", false)["ok"], false);
        assert_eq!(mark(&store, &scope, &first, "verified", true)["ok"], true);
        assert_eq!(mark(&store, &scope, &second, "verified", true)["ok"], true);
        let before = state(&store, &scope);
        let cycle = action(
            &store,
            &scope,
            json!({"action":"update","task_id":before["task_id"],"expected_revision":before["revision"],"stage_id":first,"spec":{"depends_on":[second],"acceptance":["check"]}}),
        );
        assert_eq!(cycle["ok"], false);
        assert_eq!(state(&store, &scope), before);
        let update = action(
            &store,
            &scope,
            json!({"action":"update","task_id":before["task_id"],"expected_revision":before["revision"],"stage_id":first,"text":"changed"}),
        );
        assert_eq!(update["ok"], true);
        let stage = store
            .get_stage(&scope, before["task_id"].as_str().unwrap(), &second)
            .unwrap();
        assert_eq!(stage.status, Status::Pending);
        assert!(stage.evidence.is_empty());
        assert_eq!(store.mark(&scope, 1, "verified", None)["ok"], false);
    }
    #[test]
    fn attempts_dedup_callbacks_and_live_edits() {
        let (store, scope, id, link, assignment) = prepared();
        let Reservation::New { attempt_id } = store
            .reserve_stage(&scope, &link, assignment.clone())
            .unwrap()
        else {
            panic!("new")
        };
        assert!(matches!(
            store
                .reserve_stage(&scope, &link, assignment.clone())
                .unwrap(),
            Reservation::Existing { .. }
        ));
        let mut mismatch = link.clone();
        mismatch.expected_revision += 1;
        assert!(store
            .reserve_stage(&scope, &mismatch, assignment.clone())
            .is_err());
        let current = state(&store, &scope);
        let edit = action(
            &store,
            &scope,
            json!({"action":"update","task_id":link.task_id,"expected_revision":current["revision"],"stage_id":id,"note":"editing live"}),
        );
        assert_eq!(edit["ok"], false);
        assert_eq!(mark(&store, &scope, &id, "verified", true)["ok"], false);
        store
            .bind_run(&scope, &link.task_id, &id, &attempt_id, "run-1")
            .unwrap();
        store
            .append_actual_model(&scope, &link.task_id, &id, &attempt_id, "primary".into())
            .unwrap();
        let mut other = link.clone();
        other.launch_key = "other".into();
        other.expected_revision = state(&store, &scope)["revision"].as_u64().unwrap();
        assert!(store.reserve_stage(&scope, &other, assignment).is_err());
        store
            .finish_attempt(
                &scope,
                &link.task_id,
                &id,
                &attempt_id,
                AttemptState::Completed,
                Some("done".into()),
            )
            .unwrap();
        let stage = store.get_stage(&scope, &link.task_id, &id).unwrap();
        assert_eq!(stage.status, Status::InProgress);
        assert_eq!(stage.attempts[0].actual_models, vec!["primary"]);
        assert_eq!(mark(&store, &scope, &id, "verified", true)["ok"], true);
        store.clear(&scope);
        store.set_goal(&scope, "new goal".into());
        let current = state(&store, &scope);
        assert!(store
            .finish_attempt(
                &scope,
                &link.task_id,
                &id,
                &attempt_id,
                AttemptState::Failed,
                None
            )
            .is_err());
        assert_eq!(state(&store, &scope), current);
    }
    #[test]
    fn bounds_are_atomic_and_reservations_can_fail_before_launch() {
        let limits = Limits {
            pads: 1,
            stages: 1,
            notes: 1,
            attempts: 1,
            text_bytes: 128,
            render_bytes: 80,
            page_size: 1,
            page_bytes: 65_536,
            pad_bytes: 1_048_576,
        };
        let store = ScratchpadStore::with_limits(limits).unwrap();
        let scope = scope();
        store.set_goal(&scope, "goal".into());
        let id = add(&store, &scope, "first", vec![]);
        let before = state(&store, &scope);
        assert_eq!(store.add_step(&scope, "second".into())["ok"], false);
        assert_eq!(state(&store, &scope), before);
        assert_eq!(store.note(&scope, "x".repeat(129))["ok"], false);
        assert_eq!(state(&store, &scope), before);
        assert_eq!(
            store.set_goal(
                &PadScope {
                    conversation: "another".into(),
                    owner: true
                },
                "x".into()
            )["ok"],
            false
        );
        let link = StageLink {
            task_id: before["task_id"].as_str().unwrap().into(),
            stage_id: id.clone(),
            expected_revision: before["revision"].as_u64().unwrap(),
            launch_key: "one".into(),
        };
        let assignment = ResolvedAssignment {
            role: None,
            models: vec!["primary".into()],
            reasoning_effort: None,
        };
        let Reservation::New { attempt_id } = store
            .reserve_stage(&scope, &link, assignment.clone())
            .unwrap()
        else {
            panic!("new")
        };
        store
            .release_reservation(&scope, &link.task_id, &id, &attempt_id)
            .unwrap();
        let mut retry = link.clone();
        retry.launch_key = "two".into();
        retry.expected_revision = state(&store, &scope)["revision"].as_u64().unwrap();
        let before = state(&store, &scope);
        assert!(store.reserve_stage(&scope, &retry, assignment).is_err());
        assert_eq!(state(&store, &scope), before);
        assert!(store.render_scoped(&scope).len() <= 80);
        assert!(ScratchpadStore::with_limits(Limits {
            pads: 0,
            ..Limits::default()
        })
        .is_err());
    }
    #[test]
    fn replacing_goal_reports_live_runs_and_isolates_callbacks() {
        let (store, scope, id, link, assignment) = prepared();
        let Reservation::New { attempt_id } =
            store.reserve_stage(&scope, &link, assignment).unwrap()
        else {
            panic!("new")
        };
        store
            .bind_run(&scope, &link.task_id, &id, &attempt_id, "live")
            .unwrap();
        let replacement = store.set_goal(&scope, "replacement".into());
        assert_eq!(replacement["live_run_ids"], json!(["live"]));
        assert_ne!(replacement["task_id"], link.task_id);
        assert!(store
            .bind_run(&scope, &link.task_id, &id, &attempt_id, "late")
            .is_err());
    }
    #[test]
    fn invalid_evidence_and_dangling_dependencies_are_atomic() {
        let (store, scope, id, link, _) = prepared();
        let before = state(&store, &scope);
        let bad = action(
            &store,
            &scope,
            json!({"action":"update","task_id":link.task_id,"expected_revision":before["revision"],"stage_id":id,"spec":{"depends_on":["missing"]}}),
        );
        assert_eq!(bad["ok"], false);
        let evidence = Evidence {
            criterion: 1,
            reference: "ref".into(),
            summary: "checked".into(),
        };
        let bad = action(
            &store,
            &scope,
            json!({"action":"mark","task_id":link.task_id,"expected_revision":before["revision"],"stage_id":id,"status":"verified","evidence":[evidence]}),
        );
        assert_eq!(bad["ok"], false);
        assert_eq!(state(&store, &scope), before);
    }
    #[test]
    fn page_total_bytes_unknown_fields_and_tiny_unicode_render() {
        for limit in 1..40 {
            let store = ScratchpadStore::with_limits(Limits {
                render_bytes: limit,
                ..Limits::default()
            })
            .unwrap();
            let scope = scope();
            assert!(store.render_scoped(&scope).len() <= limit);
            store.set_goal(&scope, "ёж".repeat(50));
            assert!(store.render_scoped(&scope).len() <= limit);
        }
        let store = ScratchpadStore::with_limits(Limits {
            page_bytes: 128,
            ..Limits::default()
        })
        .unwrap();
        let scope = scope();
        let goal = store.set_goal(&scope, "goal".into());
        let add = action(
            &store,
            &scope,
            json!({"action":"add","task_id":goal["task_id"],"expected_revision":goal["revision"],"text":"stage","spec":{}}),
        );
        assert_eq!(add["ok"], true);
        assert_eq!(
            action(
                &store,
                &scope,
                json!({"action":"get_stage","stage_id":add["stage_id"]})
            )["ok"],
            false
        );
        assert_eq!(state(&store, &scope)["ok"], false);
        let stale = action(
            &store,
            &scope,
            json!({"action":"mark","task_id":goal["task_id"],"expected_revision":goal["revision"],"stage_id":add["stage_id"],"status":"done"}),
        );
        assert_eq!(stale["revision"], add["revision"]);
        let store = ScratchpadStore::with_limits(Limits {
            pad_bytes: 256,
            ..Limits::default()
        })
        .unwrap();
        let goal = store.set_goal(&scope, "goal".into());
        let before = state(&store, &scope);
        assert_eq!(store.add_step(&scope, "x".repeat(256))["ok"], false);
        assert_eq!(state(&store, &scope), before);
        assert_eq!(before["revision"], goal["revision"]);
        for value in [
            json!({"action":"get","typo":true}),
            json!({"action":"add","spec":{"typo":true}}),
            json!({"action":"add","spec":{"assignment":{"typo":true}}}),
        ] {
            assert!(from_value::<PlanArgs>(value).is_err());
        }
    }
}
