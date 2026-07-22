# Albert

`Albert` is a personal, always-on assistant — built as an **assembly** of maturing
LamantinAI substrates rather than one model. It lives inside the [Octo](https://github.com/LamantinAI/octo)
event-driven runtime (the Reaction skeleton: bus, connectors, supervision), thinks
with persistent [kaeru](https://github.com/LamantinAI/kaeru) memory, and reaches the
world through connectors (Telegram, calendar, a scheduler). You talk to it; it
remembers, it reminds, and it keeps a verifiable scratchpad for multi-step work.

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
- **Self-restart** — on an owner turn, Albert can reload a connector's manifest or
  restart its whole self to apply a config change (history + memory persist).
- **Config-driven connectors** — Telegram (edge ACL + owner-only `/allow` `/deny`
  `/allowed`), a generic **CalDAV calendar** (Yandex/Fastmail/Nextcloud/iCloud/Google),
  a **scheduler**, **storage**, and **forkd** — each assembled from a small manifest.
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

## Layout

```
albert/
├── albert.toml         Albert-level config
├── soul.md system.md   persona + a compact operating prompt (hot-reloaded)
├── skills/             declarative + executable skills (<name>/SKILL.md)
├── config/             connector manifests (Telegram, calendar, storage, forkd)
├── src/                the cogitator + wiring
├── contrib/deploy/     hardened systemd unit + a Docker runbook
└── docs/               architecture, configuration, code map
```

The per-module code map is in [`docs/structure.md`](docs/structure.md).

## The long arc: PEMRR

The frame Albert grows into is **PEMRR** — Perception, Experience, Memory, Reflection,
Reaction — five layers it composes from. Today Albert is the working-first cut: a
ReAct-style cogitator over Octo, with kaeru as Memory and the scheduler for proactive
routines; Experience/Reflection are one rig tool-loop turn. Richer cognition is future
work — the agent building and reusing **its own** task structure (a self-built graph it
authors, grown from the per-turn scratchpad), **not** a LangGraph-style control-flow
engine over fixed stages. The architecture is laid out in
[`docs/architecture.md`](docs/architecture.md).

## Built on

- **[Octo](https://github.com/LamantinAI/octo)** — the Reaction runtime (bus, connectors,
  supervision, control-plane, the rig tool bridge + file tools). Pulled as a git dependency.
- **[kaeru](https://github.com/LamantinAI/kaeru)** — the Memory substrate (bi-temporal
  cognitive graph). Pulled as a git dependency, pinned to a rev.

## License

MIT — see [LICENSE](LICENSE).
