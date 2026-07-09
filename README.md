# Albert

`Albert` is a personal, always-on assistant — built as an **assembly** of maturing
LamantinAI substrates rather than one model. It lives inside the [Octo](https://github.com/LamantinAI/octo)
event-driven runtime (the Reaction skeleton: bus, connectors, supervision), thinks
with persistent [kaeru](https://github.com/LamantinAI/kaeru) memory, and reaches the
world through connectors (Telegram, calendar, a scheduler). You talk to it; it
remembers, it reminds, and it keeps a verifiable scratchpad for multi-step work.

The long-term frame is **PEMRR** — Perception, Experience, Memory, Reflection,
Reaction — the five layers Albert composes from. Today Albert is the working-first
cut: a ReAct-style cogitator over Octo, with kaeru as Memory and a scheduler for
proactive routines. Richer cognition is future work — the agent building and reusing
**its own** task structure (a self-built graph it authors, grown from the per-turn
scratchpad), **not** a LangGraph-style control-flow engine over fixed stages.

## Overview

A running Albert is an Octo runtime supervising a set of long-lived connectors, with
a single **cogitator** (the deciding layer) wired in:

- **Octo** owns the nervous system — an in-process bus of typed envelopes, connectors
  as autonomous supervised tasks, reflex/cognition split, a control-plane. Albert does
  not reimplement any of this; it is a userland `Cogitator`.
- **kaeru** is the deliberate memory — a typed, bi-temporal cognitive graph the agent
  recalls from and writes to **as a tool** (`kaeru_recall`, `kaeru_remember`, …). Not
  a memory file re-read every turn.
- **The cogitator** ties them: it perceives `chat.message` and `alarm.fired`, runs a
  rig native tool-loop with memory + scheduler + scratchpad tools, and replies.

## Features

- **Persistent memory (kaeru)** — recalls context on demand, remembers durable facts;
  a real cognitive-graph substrate, not a bulk memory blob.
- **Reminders** — a scheduler connector owns time. "Remind me every hour to drink
  water" saves a memory task + a recurring alarm; when it fires Albert recalls the task
  and messages you; when you say it is done, it marks the task done and cancels the alarm.
- **Proactive routine** — a self-scheduled memory-reflection pass (`kaeru_reflect`)
  that Albert seeds on startup and runs silently on a cadence. Its first base routine.
- **Loop scratchpad** — a super-operational, per-task working object the agent authors
  (`scratchpad_goal/step/mark/note`) and sees each turn, so multi-step tasks are
  **verifiable**: a task is done only when every step is `verified`.
- **Config-driven connectors** — Telegram channel with an **edge ACL** (unlisted chats
  dropped before the bus; owner-only `/allow` `/deny` `/allowed` reflex) and a generic
  **CalDAV calendar** organ (list/create/delete events; Yandex/Fastmail/Nextcloud/iCloud),
  each assembled from a manifest under `config/connectors/`.
- **Config-as-data** — Albert-level settings in `albert.toml`; secrets in `.env` (named
  in the TOML by their env var); persona + instructions in `soul.md` / `system.md`,
  held in RAM and **hot-reloaded by mtime**.
- **Telegram or console** — Telegram when a token is set; a stdin/stdout console
  channel otherwise for local dev.

## Quick start

Prerequisites: Rust (1.85+), an OpenRouter (or compatible) API key, and — for the real
run — a Telegram bot token and, optionally, Yandex calendar credentials.

1. **Secrets** — put them in `../.env` (repo root, next to this crate):

   ```sh
   ALBERT_OPENAI_KEY=sk-or-...            # LLM key (OpenRouter)
   OCTO_TELEGRAM_TOKEN=123456:ABC...      # Telegram bot token (omit -> console)
   OCTO_YANDEX_APP_PASSWORD=...           # Yandex APP password (for the calendar)
   ```

2. **Config** — edit `albert.toml` (model, reflection cadence, prompt/state paths) and
   the connector manifests:
   - `config/connectors/telegram/telegram.toml` — set `owner_chat` to **your** chat id
     (ask `@userinfobot`), or the ACL locks everyone out including you.
   - `config/connectors/calendar/calendar.toml` — set `login` (your Yandex address).

3. **Run**:

   ```sh
   cargo run          # from albert/albert/
   ```

   With `OCTO_TELEGRAM_TOKEN` set, Albert connects to Telegram (ACL-gated) and the
   calendar; otherwise it runs a console channel (no calendar in that dev mode).

See [`docs/`](docs/) for the architecture and the full configuration reference.

## Layout

```
albert/
├── albert.toml         Albert-level config
├── soul.md system.md   persona + instructions (hot-reloaded)
├── config/             connector manifests (Telegram, calendar)
├── src/                the cogitator + wiring
└── docs/               architecture, configuration, code map
```

The per-module code map is in [`docs/structure.md`](docs/structure.md).

## Built on

- **[Octo](https://github.com/LamantinAI/octo)** — the Reaction runtime (bus, connectors,
  supervision, control-plane). Pulled as a git dependency.
- **[kaeru](https://github.com/LamantinAI/kaeru)** — the Memory substrate (bi-temporal
  cognitive graph). Pulled as a git dependency, pinned to a release.

## License

MIT — see [LICENSE](LICENSE).
