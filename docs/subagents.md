# Scoped subagents

Albert can delegate independent tasks through the `subagent` native tool. A run
has explicitly supplied text context, its own model-pool snapshot, tool loop,
scratchpad, execution journal, cancellation scope and working directory.

This is one-level delegation: children cannot spawn more children. Orchestration
lives in Albert; Octo continues to carry ordinary connector commands and opaque
cancellation scopes. No Octo runtime policy or dependency revision changes.

## Using the tool

Call `{"action":"capabilities"}` to discover configured model IDs, advertised
connector IDs, and native tool names that can be delegated. No keys or token
values are returned. Use `connector_search` for the connector contracts.

For example, using IDs returned by discovery:

```json
{
  "action": "spawn",
  "task": {
    "task": "Find primary sources for the supplied question and summarize them.",
    "context": "The question and relevant background go here.",
    "models": ["research-primary", "research-fallback"],
    "connectors": ["search", "browser"],
    "tools": ["scratchpad_note"],
    "max_tool_turns": 8,
    "timeout_secs": 300
  }
}
```

A writer can instead receive `models: ["writer"]`, `connectors: []`, `tools: []`
and the researcher's findings as explicit context. The context is text in this
version; parent conversation history, persona and attachments are not copied.

Spawn returns a `run_id`. Other actions:

- `list`: runs visible in this source connector and conversation.
- `inspect`, with `run_id` and optional entry `offset`: compact state and a page
  of actions, previews, extraction metadata and paths to heavy payload files.
  The full journal stays in the configured history store; completed runs are
  inspected from that store. Live/unpersisted runs use their in-memory trace.
- `read`, with `run_id`, `entry`, optional `part` (`result` or `arguments`),
  `field` (e.g. `["result", "html"]`), character `offset` and `limit`: read a
  bounded portion of one journal payload. It preserves the same conversation and
  owner access checks as inspect. `next_offset` continues either form of paging.
- `wait`, with `run_id` and optional `seconds` (default 30, maximum 60): wait for
  completion and return its outcome. Repeat if still running.
- `cancel`, with `run_id`: cancel work, propagate cancellation to connectors and
  return its state.

The outcome is `completed`, `failed`, `cancelled`, or `timed_out`. Results go to
the parent tool caller. There are no automatic chat messages from children;
explicitly granting a messaging connector permits that connector's normal
operations. Inspect and verify results before using them in the final response.

## Grants and model selection

A connector grant covers the **whole connector instance**. The child's catalog
and `connector_search` contain exactly the granted advertised instances. Dispatch
checks the target before publishing, so guessing an ungranted ID does not work.
Dispatch also validates the event kind against that instance's declared input
contract; a connector grant cannot be used to forge unrelated bus events.
Runtime `octo.control.*` messages are host authority and cannot be smuggled through
a connector target. There is no per-method, domain, chat or initiative policy.

Native tools are individually selected by name, independently of connector
access. Available tools include scratchpad tools, skill discovery/application,
`read` and `write`, and the configured memory backend's verbs. Embedded memory
exposes its local verbs and, when enabled, cloud verbs; MCP memory exposes its
available adapter tools. None are installed unless explicitly granted.
An explicitly granted memory tool uses the configured shared memory backend.
Skill instructions do not grant the connectors needed to execute them.

Children receive neither self-configuration, restart, global `model_select`,
`send_file` nor the delegation tool. Owner authority is not inherited. Owner-run
results cannot be inspected or controlled from a non-owner parent turn. Runs in
another source connector/conversation are not addressable by guessing their IDs.

The ordered `models` list is copied from the host pool at spawn. Its first ID is
preferred; fallback and retries never leave that list. A single ID prevents
fallback to other models. Child selection does not change the root preference;
root selection/reload does not change a running child. API-key and subscription
models use the same existing host authentication path, including serialized OAuth
refresh. Tool-less workers can use models declaring `tools = false`. Automatic
fallback stops once a child has attempted a tool, avoiding replay of effects.

## Interrupt, cancellation and records

A new message interrupts **only the parent turn**. Children continue under their
own scopes. Pending run IDs and statuses are supplied to the next parent turn;
completed results stop appearing there after `wait` collects them. Inspecting the action index alone does not mark the
final answer as collected.
The agent can cancel an obsolete child. Explicit `/cancel` cancels the parent and
all children in that conversation, including children surviving an earlier turn.
Runtime shutdown and each child's deadline also cancel work.

Cancellation drops the model/tool loop, then emits Octo's existing cancellation
signal for that child's scope. Connectors must honor that protocol to stop remote
work. Completed and UNKNOWN tool outcomes remain inspectable. Never assume that
cancellation rolled back an external action.

Final records use the configured history backend under a separate
`subagent/<run_id>` history key. IDs include a random suffix to avoid collisions
across restarts. `journal_saved` reports whether persistence succeeded. Live run
handles are in-memory; running jobs are not resumed after a process restart.
Collected completed handles are evicted oldest-first at the retention limit;
uncollected results are retained, and spawn refuses when no slot can be reclaimed.

Native file tools use `<code_workspace>/subagents/<random-id>`, with no
process-global environment changes. Relative traversal and symlink paths are
rejected. Workspaces remain available as artifacts; cleanup is an operator task.
Connector working directories are unchanged; the child receives its native file
workspace path and must explicitly use it in scripts when forkd is granted.
This is **not an OS/network sandbox**. A granted forkd has its normal filesystem,
network and SSH access; do not grant it when that authority is unwanted.

## Limits

```toml
[subagents]
enabled = true
max_concurrent = 4
max_retained = 64
max_runs_per_turn = 8
max_tool_turns = 16
timeout_secs = 600
```

These are the defaults when the table is absent. `enabled = false` removes the
parent tool. Concurrency and retained handles are bounded across the whole
Albert process. Spawn count is bounded per parent model turn, shared across safe
provider retries. Each child may request lower tool-turn/time limits, never
higher. Model attempts/retries inherit the pool's configured bounds. With no
nested delegation, children cannot grow another task tree. These are execution
budgets, not a currency or token-spend quota.

## Inspection payload files

Inspection writes only large requested payloads to
`<code_workspace>/tool-results/<content-hash>.json` or `.txt`. Browser HTML and
text also get standalone files for analysis without JSON escaping. These are
payload artifacts, **not journal exports**. The original journal is not rewritten
or removed. Repeated inspection reuses the same content-derived paths. Artifacts
are generated only for the requested inspection page and currently retained for
operator-managed cleanup under the workspace's existing access policy.

If artifact storage fails, inspect still returns a bounded preview and an explicit
error; `subagent read` can retrieve the original payload from history. Native file
`read` can return a much larger chunk; prefer `subagent read` for selective model
context loading. Completed run handles retain their existing in-memory lifetime;
this change does not add cross-restart run discovery.

```toml
[subagents.inspection]
page_size = 20
preview_chars = 300
artifact_bytes = 8192
read_chars = 8192
```

`list` also returns compact metadata, while `wait` continues to return the final
answer. General tool-result offloading before model ingestion, artifact lifecycle
management, and conversation-window compaction remain follow-up work in
[#37](https://github.com/LamantinAI/albert/issues/37).
