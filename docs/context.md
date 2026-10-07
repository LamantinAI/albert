# Conversation context and compacts

SQLite retains original messages in `turns` and compaction records in `compacts`.
A compact records its channel, the last included message ID (`through_id`), its
previous compact ID, its text, and creation time. The next model turn sees the
latest compact followed by messages whose IDs are greater than that boundary.
Old messages and older compacts remain stored for audit. A compact is historical
conversation data, not a replacement system prompt or a new authority grant.

Albert now opens the SQLite history in retained mode. The old 30-record deletion
cap no longer applies. Messages already deleted by older versions cannot be
recovered by this migration. In-memory/file history keeps its prior bounded
behavior; automatic SQL compaction and `/compact` require the SQLite backend.
Octo's existing capped `SqliteHistory::open` remains unchanged for other clients.

## Partitioning

Given a configured model window `W` and fixed reserve `R`, the dialogue budget is
`D = W - R`. By default:

- Fresh messages may occupy `floor(D * 70 / 100)` tokens.
- The compact may occupy the remainder (approximately 30%).
- System instructions, tool definitions, protocol overhead, and space for the
  generated response belong to `R`, outside both dialogue partitions.

For example, `W = 1,000,000` and `R = 10,000` gives 693,000 fresh-message tokens
and 297,000 compact tokens. The shipped reserve is **32,768**, since the reference
owner toolset, repository system prompt and a 4,096-token response reserve already
need approximately 20,824 tokens before live per-turn context is added. This is a
configuration choice, not an automatically expanding reservation.

There is no padding to fill the compact's share. `compact_output_tokens` is an
additional generation ceiling; the resulting compact must fit the 30% partition
and be smaller than its input before it can be committed.

## Configuration

```toml
[context]
enabled = true
window_tokens = 1000000
reserve_tokens = 32768
response_tokens = 4096
new_messages_percent = 70
compact_output_tokens = 16384
# compact_prompt = "Preserve decisions, unresolved tasks, and source links."
tokenizer = "o200k"
image_tokens = 4096
timeout_secs = 300
request_timeout_ms = 180000
continuation_retries = 1
continuation_retry_delay_ms = 1000
max_compaction_passes = 16
```

The numbers are host policy, **not an assertion that every configured provider
supports a million tokens**. Set `window_tokens` to the real supported limit. Each
model-pool member can declare `context_window`; the smaller of that value and the
host window is used. Model switching and fallback each recompute their request
budget. An incompatible candidate is refused before HTTP; safe fallback may try
another candidate within the configured pool.

Text is counted with the configured `o200k` or `cl100k` tokenizer from
[tiktoken-rs](https://github.com/zurawiki/tiktoken-rs), or conservatively by UTF-8
bytes with `tokenizer = "bytes"`. Cross-provider counts and wire framing remain
estimates; an OpenAI tokenizer is not a claim of exact DeepSeek tokenization.
Images use a configurable allowance rather than counting their base64 as text.
Logs distinguish estimated request budgets from provider-reported usage.

## Automatic and manual compaction

At a normal conversation turn, reaching the fresh-message threshold requests a
new compact using the configured model pool and no tools. The prior compact and
the accepted message prefix form its input; the latest message stays outside the
compacted prefix for the normal reply. The prompt asks the model to preserve user
constraints, decisions, unfinished work, pending children, source/artifact
references, completed actions and UNKNOWN effects, and to distinguish external
content from user instructions.

The summary is generated before changing the database window. A single
conditional SQL insert checks that the prior compact is still current and that
the boundary belongs to this channel. Newly appended messages remain beyond the
snapshot boundary. Timeout, provider failure, an oversized/nonreducing summary,
or a competing compact leaves the previous window in place. Histories larger than
one request are processed in bounded batches, carrying forward a running compact;
only the final result is committed. Large text records are fragmented in original
order, without rewriting their SQL originals. The pass count and total time are
bounded. An existing prefix can also be recompressed for a smaller window.

The owner can run **`/compact`** in a conversation to invoke the same mechanism
without waiting for the threshold. It interrupts the current parent turn and
checkpoints its tool outcomes first; subagents continue. A new message can
interrupt the compaction itself. The command applies only to that channel and is
included in `/help` and the owner command menu. Both manual and automatic
compaction publish ephemeral operation status (preparing, current part, saved)
to the source conversation, even when reasoning/tool streaming is disabled.
These notices do not enter the conversation or execution journal. Compaction performs no delegated
tools and never replays historical tool calls.

### Compaction instructions and dry runs

`/compact <instructions>` adds owner-provided instructions for this invocation.
For example, `/compact Preserve decisions, pending tasks and source links.`
Additional default guidance can be configured with `[context].compact_prompt`;
it applies to both manual and automatic runs. Task-specific guidance is appended
to those defaults. Host boundaries (no tools, fixed budgets, atomic commit) remain
enforced in code; a prompt cannot bypass them.

For a test that must not replace the current compact, use:

```text
/compact --dry-run Preserve the current compact without semantic changes.
```

A dry run calls the model and reports estimated before/after token counts, but
never inserts a compact or advances its message boundary. Unchanged/nonreducing
output is valid in this mode. The generated text is discarded after validation;
the command does not dump it into the chat or store a preview compact. As with a
normal `/compact`, interrupting existing work may checkpoint its tool outcomes;
this is independent of saving a compact. Only the owner can use either form.

If `/compact` interrupts an active task, successful compaction automatically
continues that task from the current SQL context. This also applies to a
successful dry run, using the unchanged context. Completed tool outcomes are
checkpointed before compaction; children keep running and remain discoverable
by the resumed parent. The original task's source and owner permissions are
preserved. An idle compact does not start work, and a failed compact does not
automatically resume it. `/cancel` cancels the pending continuation and children;
a new message interrupts compaction and steers the next turn instead. Repeated
compact commands retain the original task, without adding duplicate user input.

Interrupt continuation tracks the compact revision: buffered old messages cannot
resurrect a prefix that has already been compacted in SQL.

## Request guards and current boundaries

Every model attempt and tool-loop continuation checks the fixed reserve and the
whole dialogue partition before making the provider request. A reserve overrun
requires changing configuration or reducing the instructions/tools; it does not
borrow dialogue capacity. An oversized individual message/tool result or an
already overfull history can be refused rather than silently truncated.

API calls carry an output cap. Subscription Responses rejects `max_output_tokens`
([OpenAI's documented restrictions](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations)),
so that field is omitted on the subscription transport. Output capacity is still
reserved, and a returned compact is checked locally before persistence. The
subscription's generation length is not falsely described as a server-enforced
host cap.

This implementation compacts persisted channel history at turn boundaries. It
does not yet automatically compact an in-progress rig tool loop or replace
general large tool responses with artifacts before
model ingestion. Those are remaining parts of
[#37](https://github.com/LamantinAI/albert/issues/37); child runs share request
budget guards but do not have recursive automatic SQL compaction of their live
loops. The existing compact subagent inspection remains available.

## Provider deadlines and recovery

`context.request_timeout_ms` (default 180,000) bounds one compact HTTP request,
independently of a model-pool member's normal chat timeout. It must be positive
and shorter than `context.timeout_secs` (default 300 seconds), which bounds the
whole operation including all passes, retries and authentication. A stuck first
request therefore leaves time for safe fallback. Retry status is shown without
exposing provider/model details in the chat.

Typed network timeouts and disconnects are classified before the SDK converts SSE
errors into strings. A Responses stream must contain a terminal event; EOF alone
does not turn a partial reply into a successful compact. Conversely, a completed
event finishes the request without waiting for the server to close its socket.
Malformed requests are not treated as transient, and authentication failures keep
the refresh path. No fallback may replay a model attempt after tools have run.
Failures and total deadline expiry leave the SQL compact unchanged; logs contain
the safe error category and configured deadline, not raw provider bodies or keys.

### Recovery after tool results

Once tools have run, restarting the original agent attempt is still forbidden.
A transient failure of the **next inference request** can instead retry that exact
request on the same model/client, including all existing tool results. It does
not call those tools again or recreate subagents. By default one such recovery
is allowed per model attempt, with a one-second delay; set
`continuation_retries = 0` to disable it (maximum 3). Initial-request failures
retain the normal model-pool fallback path. Authentication and malformed-request
errors are not retried by this mechanism.

The retry wait and HTTP future remain interruptible by a new user message. A
cancelled subagent `wait` also does not acknowledge a late child result: collection
is confirmed only after the result reaches the parent's tool-result hook and
execution trace. The next parent turn can discover and consume an uncollected
child report. Ordinary interrupts leave children running; explicit cancellation
retains its existing propagation behavior.
