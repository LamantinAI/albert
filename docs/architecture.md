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

`AlbertCogitator` (`src/cogitator.rs`) subscribes to `chat.message` and `alarm.fired`
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

## Three context tiers

Kept deliberately distinct (they are different things, with different owners):

| Tier | What it is | Owner |
|------|-----------|-------|
| **Chat transcript** | the dialogue + actions, a linear log | Octo history (`src/history.rs`) |
| **Scratchpad** | super-operational task state (goal + steps + status) | the loop (`src/scratchpad.rs`) |
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

- **Telegram** — an **edge ACL**: a message from an unlisted chat is dropped *before*
  the bus (untrusted input never reaches cognition — the prompt-injection boundary).
  Listed chats get a trust gradient + a `role` tag. The owner manages the list at
  runtime via a **deterministic** `/allow` `/deny` `/allowed` reflex (`src/acl.rs`),
  gated on the incoming message's `role == owner` — security stays out of the LLM.
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
