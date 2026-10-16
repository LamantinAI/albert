# Stage and model planning

Albert extends its existing scratchpad for substantive multi-step assignments.
It does not run a separate planner on every message. Ordinary conversation and
small one-shot answers need no plan or extra model call.

## One working scratchpad

Each source connector, conversation and owner/non-owner partition has one current
RAM scratchpad. The host supplies that identity; tool arguments cannot choose
another conversation or impersonate the owner. Simple goal/step/note tools still
work. `scratchpad_goal` replaces the task, and `scratchpad_clear` removes it.
There is no SQL plan store, permanent plan archive, TTL or restart recovery.
Existing conversation and subagent journals keep their normal history backend.

For a substantial assignment, Albert sets a goal and uses `scratchpad_plan` to
add stages with a description, dependencies, model assignment, descriptive scope
and acceptance criteria. It authors the plan for the task; no fixed workflow or
built-in analyst/researcher/builder sequence is imposed.

Read `scratchpad_plan` with `action: "get"` for the task ID, revision and a page
of stages. Use `action: "get_stage"` and `stage_id` for details. Mutations require
`task_id` and `expected_revision`; stale changes fail atomically. A later read
retrieves the current revision. IDs remain stable when another stage changes.

```json
{
  "action": "add",
  "task_id": "task-2",
  "expected_revision": 1,
  "text": "Research the supplied question",
  "spec": {
    "depends_on": [],
    "assignment": {"role": "researcher"},
    "scope": ["Research only; do not publish"],
    "acceptance": ["Findings have primary sources and disclose gaps"]
  }
}
```

The example role must be configured by the operator. Omit `assignment` to use the
current primary model. Scope text describes the assignment; actual access comes
from the explicit connector and native-tool grants on each child task.

## Models are opt-in

With no role configuration, every new child uses only Albert's **currently
selected primary model**. Another member of the root model pool is not implicitly
available for delegation or fallback. `/model` still controls the root preference.
Changing it affects new launches; running children keep their immutable snapshot.

Operators may add `[[roles]]` to the model-pool file. Each profile maps its own ID
to an ordered list of configured model IDs and an optional reasoning effort.
There are no active built-in vendor or model assignments.

- Without a role or explicit models, the default remains `[current primary]`.
- A role uses its configured ordered model list.
- Explicit models without a role must belong to the primary plus the models in
  user-configured roles. With a role, they must be a subset of that role's models.
- Explicit effort overrides the role default. Every selected model must declare
  support for that exact effort and its request format; unsupported effort fails
  before launch. Omitted effort leaves the provider default unchanged.

`subagent` capabilities exposes only child-selectable models, the current default
and configured roles. Role profiles grant no connectors, native tools or owner
privileges. See [model pool](model-pool.md) for configuration.

## Delegation and verification

A stage-linked `subagent` spawn uses a `stage` object with `task_id`, `stage_id`,
`expected_revision` and a caller-chosen `launch_key`. The stage's assignment is
canonical. Omit task model/role/effort overrides, or supply matching values.
Dependencies must be verified before a stage starts. An atomic RAM reservation
links its attempt to the child run; actual configured model attempts are recorded
separately from the requested assignment.

Retrying the same launch key and original revision/assignment returns the same
attempt rather than starting a duplicate child. Reusing it with changed inputs is
an error. A new attempt uses a new launch key and the latest plan revision.
Ordinary unbound delegation still works through the same model resolver.

New messages interrupt only the parent. Children and their stage links survive
that interruption and manual compaction in the same process. Albert should read
the current plan, list children and collect existing results before launching
replacements. Explicit cancellation and child deadlines retain their existing
semantics. Cancellation does not prove that external effects were rolled back.

A completed child does **not** verify its stage. Albert inspects the result,
records `done`, then checks acceptance and marks `verified` with evidence:

```json
{
  "action": "mark",
  "task_id": "task-2",
  "expected_revision": 5,
  "stage_id": "stage-3",
  "status": "verified",
  "evidence": [{
    "criterion": 0,
    "reference": "subagent/<run_id> and source links",
    "summary": "Reviewed the report and checked its cited sources"
  }]
}
```

Evidence indices are zero-based. The host checks that every criterion has a
nonempty reference and summary; Albert is responsible for the actual check.
Changing stage text/specification resets its current evidence and dependent
stages. Edits affecting live attempts fail until those runs finish or are
cancelled. Legacy numeric `scratchpad_mark` cannot bypass planned-stage checks.

Clear completed or abandoned work. Replacing/clearing a goal reports linked live
run IDs and does not secretly cancel them. Late callbacks from the old task cannot
recreate it or alter a new task. Explicitly collect or cancel obsolete runs.

## Bounds

`[scratchpad]` in `albert.toml` configures the shared RAM store. Defaults:

```toml
[scratchpad]
pads = 128
stages = 128
notes = 128
attempts = 32
text_bytes = 16384
pad_bytes = 1048576
render_bytes = 16384
page_bytes = 65536
page_size = 32
```

Limits cover active scopes, stages/notes per pad, attempts per stage, individual
strings, total serialized pad bytes and model-facing output. Oversized mutations
fail without partially changing state. Active plans are not silently evicted;
clear or reduce work when a limit is reached. The preamble contains a bounded
summary; detailed stage reads are explicit and bounded.
