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
  events on their own cadence (Telegram, calendar, scheduler);
- a **reflex/cognition split** — deterministic routing vs. the LLM cogitator;
- **supervision + a control-plane** — restart policies, `octo.control.*` signals.

Albert is a userland `Cogitator` — "you bring the brain." It never reimplements the
bus, supervision, or connector lifecycle.

## The cogitator loop

`AlbertCogitator` (`src/cogitator.rs`) subscribes to `chat.message` and `alarm.fired`
and, per event:

1. **Perceive** — a user message, or a scheduler fire.
2. **Reflex** — instant, no LLM: `/start` `/help`, and the owner-only ACL admin
   (`/allow` `/deny` `/allowed`, see below).
3. **Assemble context** — the base preamble (soul + system), incoming provenance
   (where the message came from), the current time, active reminders (queried from
   the scheduler), and the channel's scratchpad — plus the chat transcript history.
4. **Reason** — a `rig` native tool-loop (`max_tool_turns`) with the full toolset:
   `dispatch_to_connector` (reaches the scheduler + calendar), the kaeru verbs, and
   the scratchpad tools.
5. **React** — reply as a `chat.reply` envelope back to the source connector/channel;
   persist the exchange to history.

An `alarm.fired` is routed by its payload: a **user reminder** (`{task, channel,
reply_via}`) drives a recall-and-remind turn; a **system routine** (`{routine}`) runs
silently (see Routines).

### The action space is the connector set

The agent reaches the world only through connectors (env-as-tools). A connector that
advertises a `description` appears automatically in the cogitator's dispatch catalog —
so adding an organ (like the calendar) needs **zero cogitator change**. Albert dispatches
`calendar.list_events` / `octo.scheduler.add_alarm` / … via the one `dispatch_to_connector`
tool and awaits the correlated `<kind>.result`.

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

## Reminders

The **scheduler** connector owns time. A reminder is a recurring alarm whose payload
carries the memory-task name + the channel to remind on. On `alarm.fired` Albert
recalls the task and messages the user; on "done" it marks the kaeru task done and
dispatches `octo.scheduler.cancel_alarm`. Active alarms are front-loaded into every
chat turn so the model cancels the right one by matching the task.

## Routines (proactivity)

Same substrate, no new machinery: a **routine** is a recurring alarm keyed by
`{routine: "memory_reflection"}`. `on_alarm` routes it to a silent handler (no user
message) that runs a `kaeru_reflect` pass and acts on the maintenance work-list.
`src/routines.rs` **idempotently seeds** the base routine on startup (retry until the
scheduler is up; period from `albert.toml`).

## Connectors (config-driven)

Telegram and the calendar are assembled from manifests via Octo's `from_config_file`
(register the factory, point the builder at `config/octo.toml`):

- **Telegram** — an **edge ACL**: a message from an unlisted chat is dropped *before*
  the bus (untrusted input never reaches cognition — the prompt-injection boundary).
  Listed chats get a trust gradient + a `role` tag. The owner manages the list at
  runtime via a **deterministic** `/allow` `/deny` `/allowed` reflex (`src/acl.rs`),
  gated on the incoming message's `role == owner` — security stays out of the LLM.
- **CalDAV calendar** — a generic (RFC 4791) organ: one crate, many calendars (a
  configured instance per account). Commands `calendar.{list,create,delete}_event`.

## Settled today; future direction

Settled today: the working-first cogitator, kaeru memory, reminders + the reflection
routine, the scratchpad, config-driven Telegram (ACL) + calendar, TOML config +
hot-reloaded prompts.

**Future direction** is *not* a LangGraph-style control-flow graph engine over fixed
PEMRR stages. It is the agent building — and saving — **its own** task structure:
growing from the per-turn scratchpad toward reusable, self-authored task graphs
("this worked; keep it"). The working-first loop stays; the non-cognition layer
(memory, reminders, scratchpad, connectors) is unchanged.
