# Albert

<p align="center">
  <img src="resources/albert.png" alt="Albert" width="240">
</p>

<p align="center"><b>An always-on AI assistant that grows with you — and stays fast doing it.</b></p>

`Albert` is built as an **assembly** of maturing LamantinAI substrates rather than one
model. Talk to him one-on-one or drop him into a group chat — he serves a person or a
whole team. Three things set the shape of it:

- **It grows with you.** Albert remembers with [kaeru](https://github.com/LamantinAI/kaeru),
  its memory — it keeps learning the more you (and your group) use it, far past what a
  single conversation could hold.
- **It stays fast.** It reaches for memory only when it needs to, and pulls just what
  matters — so replies stay quick no matter how much it knows.
- **It lives in an environment.** The [Octo](https://github.com/LamantinAI/octo) runtime
  wakes it on its own — reminders and routines just happen — and makes connecting a new
  app or service easy. So Albert is proactive, works across your channels, and picks up
  new skills without a rebuild.

You talk to it; it remembers, it reminds, and it keeps a verifiable scratchpad for
multi-step work.

## Why Albert

Most assistants built on a coding-agent loop re-read a memory file and a wall of
instructions on **every** turn — so the more they know, the heavier, slower, and
pricier each reply gets. Albert is built the other way round:

- **Deliberate memory, not re-read every turn.** kaeru is a bi-temporal cognitive
  graph Albert *queries as a tool* — it recalls what's relevant on demand instead of
  stuffing a growing memory blob into every prompt. Context per turn stays small
  (lower latency, lower token cost), and memory keeps growing past what any context
  window could hold. It learns more without the prompt getting heavier.
- **Lean by design.** The system prompt is deliberately compact, and skills are a
  *catalog* (name + when-to-use) in the preamble — a skill's full instructions load
  only when it's applied. The model carries only what the turn actually needs.
- **It lives in an environment, not a chat loop.** Octo's event bus + supervised
  connectors make Albert proactive and multi-channel: reminders and routines fire on
  their own, connectors push events on their own cadence, and Albert can even restart
  its own parts to apply a config change. Not a request→response wrapper around a few
  functions.
- **Real hands.** A jailed file workspace (read/write/edit/glob/grep) with durable
  storage and file send/receive, plus a sandboxed script runner (forkd) for executable
  skills. It doesn't only talk — it fetches, transforms, and produces files.
- **Safe at the edge.** Untrusted chats are dropped *before* they reach cognition (an
  edge ACL — the prompt-injection boundary); scripts run as a lower-privileged user
  under a hardened systemd unit.

The net effect: a lighter, faster loop that stays sharp as it accumulates knowledge —
because knowledge lives in memory it recalls, not in a prompt it re-reads.

## What it does

- **Persistent memory (kaeru)** — recalls context on demand, remembers durable facts,
  reflects on a cadence; a real cognitive-graph substrate, not a bulk memory blob.
- **Reminders & calendar** — "remind me every hour to drink water" becomes a memory
  task + a recurring alarm; a one-off "remind me tomorrow at 2pm" becomes a real
  **calendar** event (a popup that syncs to your phone). When it fires Albert recalls
  the task and messages you; when you say it's done, it closes the task and cancels.
- **Proactive routines** — a self-scheduled memory-reflection pass Albert seeds on
  startup and runs silently on a cadence. Same machinery scales to more routines.
- **Files** — a jailed workspace (read/write/edit/list/glob/grep), durable **storage**
  (promote/checkout), and file exchange (send a file to you / receive one from you) —
  bytes move by reference, never pasted through the chat.
- **Skills** — a folder of `SKILL.md` recipes Albert sees as a catalog and applies on
  demand; **executable** skills bundle a script it runs in the forkd sandbox.
- **Scripts (forkd)** — run python3 / bash (with curl / wget) in a sandbox: jailed to
  the workspace, dropped privileges, clean environment, wall-clock timeout.
- **Loop scratchpad** — a super-operational, per-task working object the agent authors
  (`scratchpad_goal/step/mark/note`) and sees each turn, so multi-step work is
  **verifiable**: a task is done only when every step is `verified`.
- **Self-lifecycle control** — on an owner turn, Albert manages its own runtime: it can
  read and edit its own config/prompt files and then apply the change by restarting —
  reloading a connector's manifest, or its whole self (history + memory persist).
- **Config-driven connectors** — Telegram (edge ACL + owner-only `/allow` `/deny`
  `/allowed`), a generic **CalDAV calendar** (Yandex/Fastmail/Nextcloud/iCloud/Google),
  a **scheduler**, **storage**, **forkd**, **search** (web search over several engines —
  DuckDuckGo now, Yandex next; the manifest sets the default and Albert can pick per
  query), and an optional **mail** organ (IMAP/SMTP, off by default) — each assembled
  from a small manifest.
- **Two brains, your call** — an **API key** (OpenRouter / any OpenAI-compatible
  endpoint; the default) or a **ChatGPT subscription** (via the Codex backend).
- **Config-as-data** — Albert-level settings in `albert.toml`; secrets in `.env`,
  named in the TOML by their env var (never the value); persona + a compact operating
  prompt in `soul.md` / `system.md`, held in RAM and **hot-reloaded by mtime**.

## How it fits together

A running Albert is an Octo runtime supervising a set of long-lived connectors, with a
single **cogitator** (the deciding layer) wired in:

- **Octo** owns the nervous system — an in-process bus of typed envelopes, connectors
  as autonomous supervised tasks, a reflex/cognition split, a control-plane. Albert
  does not reimplement any of this; it is a userland `Cogitator`.
- **kaeru** is the deliberate memory — a typed, bi-temporal cognitive graph the agent
  recalls from and writes to **as a tool** (`kaeru_recall`, `kaeru_remember`, …). Not
  a memory file re-read every turn.
- **The cogitator** ties them: it perceives `chat.message` and `alarm.fired`, runs a
  rig native tool-loop over its full toolset (memory, scheduler, calendar, files,
  storage, skills, scripts, and — for the owner — restart), and replies.

## Quick start

Prerequisites: Rust (1.85+), an OpenRouter (or compatible) API key, and — for the real
run — a Telegram bot token and, optionally, calendar credentials.

1. **Secrets** — put them in `../.env` (repo root, next to this crate):

   ```sh
   ALBERT_OPENAI_KEY=sk-or-...            # LLM key (OpenRouter)
   OCTO_TELEGRAM_TOKEN=123456:ABC...      # Telegram bot token (omit -> console)
   OCTO_YANDEX_APP_PASSWORD=...           # calendar APP password (basic-auth providers)
   ```

2. **Config** — edit `albert.toml` (model, timezone, cadence, paths) and the connector
   manifests:
   - `config/connectors/telegram/telegram.toml` — set `owner_chat` to **your** chat id
     (ask `@userinfobot`), or the ACL locks everyone out including you.
   - `config/connectors/calendar/calendar.toml` — set your account (`login`, or the
     Google `collection` + client).

3. **Run**:

   ```sh
   cargo run          # from albert/albert/
   ```

   With `OCTO_TELEGRAM_TOKEN` set, Albert connects to Telegram (ACL-gated) and the
   calendar; otherwise it runs a stdin/stdout console channel for local dev.

See [`docs/`](docs/) for the architecture and the full configuration reference, and
[`docs/configuration.md`](docs/configuration.md) for a complete, copyable working config.

## The long arc: PEMRR

The frame Albert grows into is **PEMRR** — Perception, Experience, Memory, Reflection,
Reaction — five layers it composes from. Today Albert is the working-first cut: a
ReAct-style cogitator over Octo, with kaeru as Memory and the scheduler for proactive
routines; Experience/Reflection are one rig tool-loop turn. Richer cognition is future
work — the agent building and reusing **its own** task structure (a self-built graph it
authors, grown from the per-turn scratchpad), **not** a LangGraph-style control-flow
engine over fixed stages. The architecture is laid out in
[`docs/architecture.md`](docs/architecture.md).

## Roadmap

Albert is early and moving fast. Near-term:

- **More integrations.** More connectors — Albert's action space *is* its set of
  connectors, and each is a small manifest, so new systems (messaging, task trackers,
  home/devices, more calendars and mailboxes) slot in without touching the core.
- **More model providers.** Beyond an API key and a ChatGPT subscription — first-class
  support for more providers and their subscription plans, chosen from config.
- **An admin panel.** A UI for setup and day-to-day configuration (connectors, secrets,
  persona, skills), so running your own Albert doesn't mean hand-editing TOML.

Longer-term cognition work is sketched in **The long arc** above and in
[`docs/architecture.md`](docs/architecture.md).

## Built on

Albert is assembled from three LamantinAI substrates rather than reimplemented. Each
earns its place — and each is what makes the advantages above cheap to have:

- **[Octo](https://github.com/LamantinAI/octo)** — the Reaction runtime, the reason
  Albert *lives in an environment* instead of a chat loop. Connectors are autonomous,
  supervised **in-process tasks** (one process, no IPC tax) that push events on their
  own cadence, so the assistant is proactive and multi-channel by construction. Its
  **env-as-tools** model means the agent's action space *is* the set of registered
  connectors — add an organ (calendar, storage, forkd) and it appears in the toolset
  with **zero cogitator change**. A reflex/cognition split keeps security and routing
  deterministic (off the LLM), and a **control-plane** lets Albert restart a connector
  or its whole self. Octo also carries the rig tool bridge and **octo-code** — a
  separate set of file tools (`read`/`write`/`edit`/`list`/`glob`/`grep`) jailed to the
  workspace, so Albert gets a coding-agent's hands on the filesystem without leaving
  the runtime.
- **[kaeru](https://github.com/LamantinAI/kaeru)** — the Memory substrate, the reason
  the prompt stays light as Albert learns. Not a memory file but a typed, **bi-temporal
  cognitive graph** with operational and archival tiers, reached entirely **as tools**
  (recall / remember / reflect / link / synthesise, filed into named *initiatives*).
  Recall is selective, so context per turn stays small and memory scales far past any
  context window; being bi-temporal, it records both when something happened and when
  it was learned. Optional cloud endpoints let it share to / pull from team knowledge.
- **[rig](https://github.com/0xPlaygrounds/rig)** — the LLM tool-calling loop Albert
  drives, with a custom HTTP layer that also speaks the OpenAI Codex backend so the
  same agent runs on an API key **or** a ChatGPT subscription.

Octo and kaeru are pulled as git dependencies (one repo each, many crates, pinned to a
rev); there is no local sibling-checkout requirement.

## License

**Business Source License 1.1** — see [LICENSE](LICENSE), with a plain-language
summary in [LICENSING.md](LICENSING.md).

Albert is *source-available*: read it, modify it, run it in production, and embed
it as a component or tool inside your own systems and products — including what
you sell. The one thing you may not do is operate Albert itself as a hosted /
managed / SaaS platform for third parties. Each released version converts to the
**Apache License 2.0** four years after it ships (the Change Date in `LICENSE`).
Contributions are accepted under the [Contributor License Agreement](CLA.md).

Version v0.1.0 was released under the MIT License and remains available under it;
the move to BSL applies from the relicensing commit forward and is not
retroactive.
