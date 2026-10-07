# Cogitator model pool

Issue #30 adds model selection and bounded fallback inside Albert's cogitator.
Octo continues to own transport and connectors. The earlier August delivery note
mentions work in progress, but no pool implementation was found in the available
Git branches/history or project memory. The former `~/claude-backup` directory
was unavailable during this implementation.

## Configure

Copy `config/models.toml.example` to `config/models.toml`, replace its placeholder
model identifiers, and add a **top-level** setting in `albert.toml`:

```toml
model_pool = "config/models.toml"
```

Paths resolve relative to `albert.toml`. Without this setting, the existing
`model`, `auth`, `base_url`, API key and vision settings become a one-member pool
named `default`. Existing installations therefore need no configuration migration.
With a pool, keep the existing top-level configuration for inherited defaults;
missing API credentials do not prevent startup but make that member unavailable.

The pool file contains `default`, `max_attempts` (default 3), `retries_per_model`
(default 0), `retry_delay_ms` (default 2000), and ordered `[[models]]` entries:

| Field | Meaning |
|-------|---------|
| `id` | Unique local alias; letters, digits, underscores, hyphens |
| `model` | Actual model identifier sent to the provider |
| `provider` | `api_key` (OpenRouter-compatible chat completions) or `subscription` (Codex Responses) |
| `base_url` | Optional endpoint override; otherwise inherits the corresponding existing endpoint |
| `api_key_env` | Optional environment variable **name**; absent inherits the existing resolved API key |
| `vision` | Required boolean: this member accepts images |
| `tools` | Required boolean: this member supports tool calling |
| `request_timeout_ms` | Per HTTP request timeout, including response streaming; default 120000 |

Secrets are never stored in the pool file or displayed by its commands.
Subscription entries share the existing OAuth manager and auth file, including
its serialized token refresh with speech connectors. Pool reload does not change
the process environment or load a new `.env` file.

There may be 1–32 members, 1–32 total attempts, 0–8 retries per member, up to 60s
between retries, and a request timeout between 1ms and 30 minutes. Invalid pool
configuration fails startup; an invalid live reload leaves the current pool intact.

## Switch without restarting

Only the owner can use these deterministic commands:

- `/model` or `/model list`: show the preferred model and configured capabilities.
- `/model <id>`: change the global preference for subsequent model turns.
- `/model reload`: atomically reload the pool file. Preserve the selected ID if it
  still exists; otherwise use the file's `default`.

The owner can also ask in ordinary language, for example “switch to codex”.
Owner turns expose the `model_select` tool: omit `model_id` to inspect configured
IDs and capabilities, then pass a valid `model_id` to select it. Guest turns do not
receive the tool, even if the model fabricates a call to it. The tool cannot edit
pool configuration, reload it, or add arbitrary models. Its result explicitly
states that the current turn is unchanged and no restart is needed. This tool is
for the owner's request, not autonomous task delegation.

The preference is in memory; process startup uses the file's `default`. It applies
to all chats and scheduled model work. It does not grant tools or owner privileges.

A running LLM/tool loop holds an immutable snapshot of the pool and its preference.
A command does not cancel it or replace its provider mid-request. Input still in
preflight/hearing uses the preference in effect when its LLM loop starts. A new
addressed message retains Albert's normal interrupt/resume behavior and its next
model loop picks up the new preference. History, memory, scratchpad, cancellation
scope and the original turn's tool permissions are preserved.

## Fallback policy

Try the preferred model, then the other members in file order. Skip members without
tool calling, and require vision when **any supplied history or prompt** contains
an image. A member's successful text response is not judged for subjective quality.
An empty answer, malformed provider response or explicit capability/context-length
mismatch can trigger fallback; generic invalid requests and tool errors do not.

Provider overload, rate limiting and transport timeouts/errors can retry the same
member up to `retries_per_model`, then try the next. Missing credentials, rejected
credentials and unavailable models also permit fallback. A live subscription token
rejection permits one forced refresh per turn. Every actual attempt, including a
refresh retry, counts against the shared `max_attempts` limit. Skipping an ineligible
member makes no network call. Exhaustion produces a clear error with safe reasons.

**Never automatically replay a model-generated tool round.** Once the current
attempt has produced a tool call, a later failure stops the automatic retry/fallback
chain and reports that external effects may have happened. Complete results and
unknown-outcome checkpoints remain in the transcript for explicit continuation.
Already-completed hearing is input context and does not itself block fallback.
The application journal is provider-neutral: model changes read the same text,
images and recorded tool outcomes, not another provider's continuation protocol.
See [journal and continuation state](model-history.md).

Model IDs, attempt counts, skipped capabilities and safe failure reasons appear
in logs only; routine model selection does not clutter the chat status. No raw provider
error body or credential is included in these messages. Automatic fallback does
not overwrite the owner's preferred model for future turns.

## Validation

Tests use local HTTP provider fixtures, without contacting paid model services:
503 and capability fallback, request timeout, owner selection between live calls,
identical history/tool permissions on fallback, and refusal to replay a real
scratchpad action after a provider failure. Policy tests cover snapshot isolation,
atomic reload, capability filtering, exhausted budgets and bounded token refresh.

File delivery keeps the host-provided origin separate from the destination. Dispatch
uses the current chat only as the default for its connector; an explicit `channel`
can target another chat or connector. Telegram file sends return correlated delivery
results, and the native `send_file` tool awaits those results when advertised. The
temporary Albert-side ban on dispatching `chat.send_file` has been removed.
