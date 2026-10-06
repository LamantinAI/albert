# Architecture

Albert is an **assembly**, not a monolith: the [Octo](https://github.com/LamantinAI/octo)
runtime (Reaction), [kaeru](https://github.com/LamantinAI/kaeru) (Memory), and a
cogitator that ties them, plus connectors for the rest.

## The frame: PEMRR

The long-term decomposition is five layers — **P**erception (what is happening),
**E**xperience (what it means now), **M**emory (what is stored), **R**eflection (what
is learned), **R**eaction (what is done). Octo is the Reaction skeleton; kaeru is
Memory; Experience/Reflection live in the cogitator. Today those middle layers are
one rig tool-loop turn. Deeper cognition — the agent building and reusing its own
task structure — is future work: a self-built graph the agent authors (grown from
the per-turn scratchpad), **not** a LangGraph-style control-flow engine over fixed
PEMRR nodes.

## Octo — the skeleton (not the brain)

Octo is the environment an agent lives in, not the agent. It provides:

- an in-process **bus** of fixed-shape `Envelope`s (HTTP/NATS-shaped header +
  opaque payload), routed by header fields with glob-matched kinds;
- **connectors** — autonomous, supervised tasks that own their lifecycle and push
  events on their own cadence (Telegram, calendar, scheduler, storage, forkd);
- a **reflex/cognition split** — deterministic routing vs. the LLM cogitator;
- **supervision + a control-plane** — restart policies, `octo.control.*` signals
  (which is what lets Albert restart a connector, or its whole self, from a tool).

Albert is a userland `Cogitator` — "you bring the brain." It never reimplements the
bus, supervision, or connector lifecycle.

## The cogitator loop

`AlbertCogitator` (`src/cogitator/`) subscribes to `chat.message` and `alarm.fired`
and, per event:

1. **Perceive** — a user message (with any attached file dropped into the workspace
   `inbox/`), or a scheduler fire.
2. **Reflex** — instant, no LLM: `/start` `/help`, and the owner-only ACL admin
   (`/allow` `/deny` `/allowed`, see below).
3. **Assemble context** — the base preamble (soul + system), incoming provenance
   (where the message came from), the current time (in the owner's `timezone`),
   active reminders (queried from the scheduler), the **skill catalog** (name +
   when-to-use, not the bodies), and the channel's scratchpad — plus the chat
   transcript history.
4. **Reason** — a `rig` native tool-loop (`max_tool_turns`) with the full toolset:
   `dispatch_to_connector` (reaches the scheduler, calendar, storage, forkd), the
   **kaeru verbs**, the **scratchpad** tools, the **file workspace** tools
   (`read/write/edit/list/glob/grep`) + `send_file`, the **skill** tools
   (`skill_list/apply/file`), and — **only on an owner turn** — the `restart` tool.
5. **React** — reply as a `chat.reply` envelope back to the source connector/channel;
   persist the exchange to history. If `stream_status` is on, the agent's tool calls
   and thoughts stream into one in-place-edited status message while it runs, deleted
   at end-of-turn (a typing indicator is always on).

An `alarm.fired` is routed by its payload: a **user reminder** (`{task, channel,
reply_via}`) drives a recall-and-remind turn; a **system routine** (`{routine}`) runs
silently (see Routines).

### The action space is the connector set

The agent reaches the world only through connectors (env-as-tools). A connector that
advertises a `description` appears automatically in the cogitator's dispatch catalog —
so adding an organ (like storage or forkd) needs **zero cogitator change**. Albert
dispatches `calendar.list_events` / `octo.scheduler.add_alarm` / `storage.put` /
`forkd.run` / … via the one `dispatch_to_connector` tool and awaits the correlated
`<kind>.result`. (The file, `send_file`, skill, and `restart` tools are the
exceptions — native rig tools the host binds directly, not connector dispatch.)

## Interrupting a conversation

Albert decides what a new chat event means; Octo does not impose an interruption
policy on other cogitators. A message in the same conversation interrupts the
current model/tool future, sends `octo.control.cancel` for that attempt's unique
scope and resumes with the complete working context plus the new input.
Different connector/channel pairs have separate interruption gates. An unfinished
trusted-user task cannot gain owner tools just because the owner sends the next
message.

Accepted user messages enter history **before** model execution. Completed tool
rounds retain their full results; outstanding calls get explicit unknown-outcome
results, and calls not yet dispatched are marked not executed. Complete call/result
pairs are stored together, so trimming the rolling history cannot leave a dangling
tool call. Pending images and skill instructions survive interruptions in memory;
completed conversations retain the existing text representation of user media.
Secret-setting arguments are redacted from stored tool traces.

The reply commit and accepting a new input are serialized. Once an interrupt wins,
the old task cannot publish a stale final reply. `/cancel` stops without launching
a continuation, while keeping accepted messages and the tool checkpoint. Provider
retries are allowed only before any tool calls have been issued.

Voice transcription runs inside the scoped turn, so it does not hold up incoming
chat events. Periodic routines and scheduled reminders do not block the intake
loop. A connector owns its cancellation mechanics: forkd terminates its process
group; speech, transcription and image generation cancel their I/O. Cancellation
cannot roll back external effects, and interrupted writes are not automatically
replayed.

## Three context tiers

The memory tier has a dedicated backend boundary (`src/memory/`): embedded
`kaeru-rig`, or a kaeru MCP session over Streamable HTTP/stdio. Both install
`kaeru_*` verbs directly in the reasoning toolset. External memory is not a
world-facing Octo connector or a generic MCP tool catalog. Before the runtime
starts, an idempotent application migration materialises the `albert` initiative
if it is absent. A remote backend never opens the local vault as a fallback.

Kept deliberately distinct (they are different things, with different owners):

| Tier | What it is | Owner |
|------|-----------|-------|
| **Chat transcript** | the dialogue + actions, a linear log | Octo history (`src/history/mod.rs`) |
| **Scratchpad** | super-operational task state (goal + steps + status) | the loop (`src/scratchpad/mod.rs`) |
| **Memory** | durable, on-demand recall/write | kaeru, reached as a tool |

kaeru is **operational + persistent** memory the agent queries; the scratchpad is
**super-operational** task state the agent authors and sees each turn — it makes
multi-step work verifiable (a task is done only when every step is `verified`), and is
cleared on completion (consolidate anything durable to kaeru first). Neither is the
system prompt: persona + instructions live in `soul.md` / `system.md`, held in RAM and
hot-reloaded by mtime.

## Files: the workspace, storage, and file exchange

Albert can work with files, not just text. Three pieces, one shared directory:

- **The workspace** — an ephemeral scratch directory (`$OCTO_CODE_WORKSPACE`,
  `[code] workspace`). The octo-code file tools (`read/write/edit/list/glob/grep`,
  ported from a reference coding-tools crate) are **jailed** to it — every path is
  workspace-relative, escapes are rejected. Files a user sends arrive here under
  `inbox/`.
- **Durable storage** — the `storage` connector (`storage.put/get/list/delete` over
  string keys, plus `storage.promote` to shelf a workspace file and `storage.checkout`
  to bring it back). The workspace is throwaway; storage is what survives it. Backend
  is swappable (a local directory now, S3 later).
- **File exchange** — `send_file` mails a workspace file to the user by reference (the
  bytes move through the shared workspace, never through the model / the chat).

The workspace is **one named directory** all three fs-touching organs inherit —
octo-code writes to it, forkd runs with it as cwd, storage promotes/checkouts against
it — so a script edits the very files the agent wrote, with zero path coordination.

## Skills

A **skill** is a folder `<name>/SKILL.md`: YAML frontmatter (`name`, `description`) +
an instruction body, optionally bundling resource files. Design intent: only the
**catalog** (name + when-to-use) sits in the preamble each turn; a body loads on
demand.

- `skill_list` re-lists the catalog; `skill_apply <name>` returns the body (kept hot
  in an LRU read-through cache of `[skills] cache` entries), which the agent then
  follows **literally**; `skill_file` reads a bundled resource **in place** (never
  copied through the model or the workspace).
- A **declarative** skill is instructions only. An **executable** skill names a
  bundled script the agent runs in place via forkd (`skill_path`) — see Scripts.

## Scripts (forkd) and executable skills

The **forkd** connector runs scripts in a sandbox: `forkd.run { script | path |
skill_path, interpreter?, args?, stdin?, timeout_secs? }` → `{ exit_code, stdout,
stderr, timed_out }`. It is the *doing* half of a task (fetch a page, transform a
file, a quick computation) while the file tools handle reading and writing. An
executable skill runs its bundled script **in place** with `skill_path` — the runner
reaches it from the skills dir directly, and the workspace stays the cwd so outputs
land where the file tools can see them.

## Isolation (three layers)

Scripts are only *conditionally* trusted, so confinement is layered — and each layer
guards a different boundary; none replaces another:

- **L1 — host ← agent** (`contrib/deploy/albert.service`): Albert runs as the
  unprivileged `albert` user under systemd hardening (`ProtectSystem=strict`,
  `ProtectHome`, `PrivateTmp`, a bounded capability set). Its own code/config/skills
  are read-only at runtime; only its state is writable.
- **L2 — agent ← scripts** (forkd v0): a script runs as a **separate, lower-privileged
  user** (`albert-scripts`, dropped via setuid from the service's `CAP_SETUID`), with a
  cleared environment (no agent secrets; only `PATH`/`HOME`/`TMPDIR`/`LANG` kept), cwd
  jailed to the workspace, a wall-clock timeout that SIGKILLs the whole process group,
  and CPU/file-size rlimits. (A future stage adds a bwrap mount-namespace.)
- **L3 — per-skill capabilities** (future): a skill declaring what it may touch.

The workspace is the deliberate **handoff surface** between L1 and L2: agent writes
land group-shared (`0660`) in a setgid `2770` directory (`UMask=0007`), so the dropped
script user — a member of the shared group — can read and edit what the agent wrote.

## Self-restart (the control plane)

Because config is read at startup, Albert can apply its own config changes: the
owner-only `restart` tool emits an `octo.control.*` signal the runtime's control
listener carries out — `restart { target: "<connector id>" }` re-spawns one connector
(reload its manifest); `restart { target: "process" }` cancels the shutdown token for a
graceful stop, and systemd `Restart=always` revives the process with fresh config
(history and the kaeru vault persist across it). The tool is added to the toolset
**only on an owner turn** (`acl::is_owner`) — a non-owner never sees it.

## Reminders

The **scheduler** connector owns time. A reminder is a recurring alarm whose payload
carries the memory-task name + the channel to remind on. On `alarm.fired` Albert
recalls the task and messages the user; on "done" it marks the kaeru task done and
dispatches `octo.scheduler.cancel_alarm`. Active alarms are front-loaded into every
chat turn so the model cancels the right one by matching the task. (A one-off reminder
the user asks for is preferentially a real **calendar** event — a popup that also syncs
to their phone — with the scheduler as the in-chat-nag / fallback path.)

## Routines (proactivity)

Same substrate, no new machinery: a **routine** is a recurring alarm keyed by
`{routine: "memory_reflection"}`. `on_alarm` routes it to a silent handler (no user
message) that runs a `kaeru_reflect` pass and acts on the maintenance work-list.
`src/routines.rs` **idempotently seeds** the base routine on startup (retry until the
scheduler is up; period from `albert.toml`).

## Connectors (config-driven)

Every connector is assembled from a manifest via Octo's `from_config_file` (register
the factory in `main.rs`, point the builder at `config/octo.toml`):

- **Telegram** — transport checks chat and sender access independently and emits
  verified source metadata (chat/author IDs, chat type, addressing signal). The
  deterministic Albert reflex (`src/acl/mod.rs`) drops unaddressed group traffic
  before cognition or cancellation. ACL controls are owner/admin-only; opening
  a group with `/groupmode all` is owner-only and gives other participants guest
  access. The connector rechecks the sender before changing its persisted ACL.
  Bootstrap commands in unlisted groups are restricted to owner/admin and never
  become model turns. Source metadata accompanies each prompt and stored user turn.
- **CalDAV calendar** — a generic (RFC 4791) organ: one crate, many calendars (a
  configured instance per account, basic-app-password or OAuth2). Commands
  `calendar.{list,create,delete}_event`.
- **Scheduler** — alarms (interval / oneshot), persisted to disk; the time organ
  behind reminders and routines.
- **Storage** — the durable object store (see Files).
- **forkd** — the sandboxed script runner (see Scripts).
- **search** — web search: `search.web { query, limit?, engine? }` → a clean
  `[{ title, url, snippet }]` list (never raw HTML). It fronts **several engines at
  once**: the manifest declares them (`[connector.engines.*]`) with a `default_engine`,
  and the agent overrides per call with `engine` — DuckDuckGo now (no account/key),
  Yandex Search API next, behind one trait. **The DDG engine links the system libcurl**
  (`libcurl4-openssl-dev` to build, `libcurl.so.4` to run) — DDG's anti-bot rejects
  reqwest's TLS fingerprint with a 202 challenge, and a vendored libcurl's handshake
  outright, while the system one passes.
- **jira** — a **configurable** (`type = "http"`) organ: the whole Jira REST v2 surface
  as `jira.cmd.*` commands from a manifest alone, no code (Bearer PAT via `JIRA_TOKEN`).
  The generic `http` connector; `base_url` is a per-deployment placeholder in git. Paired
  with a `jira` skill that teaches safe use (read freely, write on request). Ported from
  a colleague's PR.
- **mail** (optional, **off by default**) — an IMAP-read + SMTP-send mailbox organ:
  `mail.cmd.{list,read,send,reply}`, attachments as metadata only, basic-auth providers
  (no Gmail/XOAUTH2 yet). The factory is registered but instantiates only when a
  `config/connectors/mail/mail.toml` is present (the repo ships just `.example`).

## Settled today; future direction

Settled today: the working-first cogitator; kaeru memory (local, with optional cloud
sharing); reminders + the reflection routine; the scratchpad; the file workspace +
durable storage + file exchange; declarative **and** executable skills over a
sandboxed forkd runner (three isolation layers, L1+L2 in place); owner-gated
self-restart; SQLite-persisted history; config-driven Telegram (ACL) + calendar +
scheduler + storage + forkd; TOML config + hot-reloaded prompts; API-key **or**
ChatGPT-subscription auth.

**Future direction** is *not* a LangGraph-style control-flow graph engine over fixed
PEMRR stages. It is the agent building — and saving — **its own** task structure:
growing from the per-turn scratchpad toward reusable, self-authored task graphs
("this worked; keep it"). Nearer-term: forkd's L2 bwrap mount-namespace and L3
per-skill capabilities; more connectors + web search. The working-first loop stays;
the non-cognition layer (memory, reminders, scratchpad, files, connectors) is
unchanged.

### Finding a connector

The `dispatch_to_connector` definition retains the complete advertised connector
catalog. Albert does not duplicate that list in the per-turn preamble. `system.md`
states the routing policy: use an appropriate configured connector before a network
script; if the task is unfamiliar, look it up before inventing an integration.

The read-only `connector_search` tool searches advertised IDs and descriptions,
with task hints for core web/media commands. Empty query lists every advertised
target (no pagination cutoff); a target-ID query returns its full public contract.
Unadvertised control commands, manifests and credentials are not exposed, and
discovery neither grants access nor promises a connector is currently healthy.
