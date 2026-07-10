# AGENTS.md — Albert

Instructions for an AI agent (or human) working in this repository. Read this
before changing code.

## Project Summary

`Albert` is a personal, always-on assistant built as an **assembly**: it runs as a
single `Cogitator` inside the [Octo](https://github.com/LamantinAI/octo)
event-driven runtime (the Reaction skeleton — bus, connectors, supervision),
thinks with persistent [kaeru](https://github.com/LamantinAI/kaeru) memory, and
reaches the world through connectors (Telegram, calendar, a scheduler). Talk to it;
it remembers, reminds, and keeps a verifiable scratchpad for multi-step work.

Single binary crate, Rust edition 2021 (toolchain 1.85+). Octo and kaeru are
**git dependencies** (see `Cargo.toml`) — one repo each, many crates; there is no
sibling-checkout requirement.

Deeper design docs: [`docs/`](docs/) (architecture / configuration / structure /
deploy). This file is the working rules; `docs/` is the reference.

## How To Work In This Repository

Orient by module before changing code. Everything is under `src/`:

| Module | Owns |
|--------|------|
| `main.rs` | Wiring: load `.env` + `albert.toml`, open the kaeru vault, build history / scheduler / scratchpad / prompt loader; register connector factories + `from_config_file`, or fall back to console; run Octo. |
| `cogitator.rs` | `AlbertCogitator` — the `Cogitator`: perceive → reflex → assemble-context → rig tool-loop → reply, for `chat.message` and `alarm.fired`. Builds the agent + toolset. |
| `acl.rs` | Owner-only Telegram ACL admin reflex (`/allow` `/deny` `/allowed`) — deterministic, no LLM in a security action. |
| `routines.rs` | Proactive routines: idempotently seed the base memory-reflection alarm on startup. |
| `scratchpad.rs` | The loop scratchpad: channel-keyed store + the `scratchpad_*` rig tools. |
| `prompt.rs` | `PromptFiles` — load `soul.md` + `system.md` into RAM, hot-reload by mtime. |
| `config.rs` | `Config` — parse `albert.toml` (serde + `toml`), resolve the LLM secret from env, resolve relative paths against the config dir. |
| `history.rs` | Bridge to Octo's `octo-history`: the per-channel transcript (hot-context tier). |
| `console.rs` | `ConsoleConnector` — stdin/stdout channel, the Telegram stand-in for dev. |
| `error.rs` | The crate error type — explicit `#[from]` variants, no `anyhow`. |

Rules of thumb:

1. Connectors (Telegram, calendar) are **config-driven** through Octo's
   `from_config_file` + manifests under `config/connectors/`. Add/adjust a
   connector there, not in code (registering its factory in `main.rs` is the only
   code touch).
2. A connector that advertises a `description` appears automatically in the
   cogitator's dispatch catalog — the agent reaches it via the one
   `dispatch_to_connector` tool. Adding an organ needs **zero cogitator change**.
3. Octo and kaeru are the source of truth for their types — consume them, don't
   duplicate. Bump the pinned rev/tag in `Cargo.toml` to move either forward.

## Local Runbook

- `cargo build` — quick check after changes.
- `cargo test` — run tests (e.g. the scratchpad store test).
- `cargo run` — run Albert. Telegram if `OCTO_TELEGRAM_TOKEN` is set, else a
  console channel. Config from `albert.toml`; secrets from `.env`.
- `cargo build --release` — the deploy binary (optimized profile: LTO + strip).

Deployment (build here, ship the binary; the target is small): see
[`docs/deploy.md`](docs/deploy.md).

## Module Organisation

Keep code **modular, grouped by purpose**, one concern per file. **Cap each file at
~500 lines (hard limit 600).** Split a module when:

- it passes ~500 lines, or
- more than a few cohesive concerns accumulate in one file.

`cogitator.rs` was split this way — the ACL admin moved to `acl.rs`, routine seeding
to `routines.rs`. When a module grows past a flat file, prefer a
`mod.rs`-with-submodules layout and keep shared cross-submodule types in the parent
`mod.rs`.

## Import Rule (strict)

- **Import the final entity, in full, by name** — structs, enums, functions, traits,
  constants. `Duration::from_secs(..)`, not `std::time::Duration::from_secs(..)`;
  `info!(..)`, not `tracing::info!`; `Utc::now()`, not `chrono::Utc::now()`.
- **Group items from the same module/crate path in braces**: one
  `use std::{sync::Arc, time::Duration};`, one `use octo_core::{...};`, one
  `use crate::{...};` — never one `use` line per item from the same path.
- **No module paths in code bodies.** Import the leaf; don't `use module` and then
  write `module::Type::method` throughout. Associated fns on an imported type
  (`Duration::from_secs`) are fine — the type is imported.
- **No glob imports** (`use foo::*`) in implementation code.
- **On name collisions, alias** the leaf (`use rig::http_client::Error as HttpError;`)
  or use a leading `::` to disambiguate an extern crate from a same-named local
  module (`use ::config::Config;`).
- **Group blocks**: std, then third-party crates, then `crate::` — blank line
  between groups.
- Reasonable exemptions: attribute macros stay pathed (`#[tokio::main]`).

In short: explicit, brace-grouped imports; no long module paths scattered through
the code body.

## Conventions

- **No emoji in code or logs.** Strip pictographic emoji from source and log
  output; arrows / quotes / bullets are fine. (The *bot's chat replies* carry an
  in-character emoji persona — that lives in `soul.md`, not in code, and is a
  deliberate product choice, not a violation of this rule.)
- **No `anyhow`.** Errors are explicit — `error.rs` defines `Error` (a `thiserror`
  enum with `#[from]` variants) and `Result<T>`. Add a variant for a new failure
  mode; don't stuff context into an existing one or `format!` at call sites.
- **Commit style**: Conventional-Commits-style lowercase prefixes — `feature:`,
  `fix:`, `chore:`, `docs:`, `git:` (note `feature`, not `feat`). No
  `Co-Authored-By` trailer.
- **Config-as-data**: nothing tunable is hardcoded. Albert-level config in
  `albert.toml`; connector manifests under `config/`; persona + instructions in
  `soul.md` / `system.md` (hot-reloaded). Secrets live only in the environment,
  named in the TOML by their env-var name.

## Secrets & per-target config

- The repo ships **placeholder** manifests (`owner_chat`, Yandex `login`). Real
  values live only on the target machine, never committed.
- Runtime state is gitignored: `state/` (scheduler alarms) and
  `config/connectors/telegram/telegram_acl.json` (the mutable ACL).

## Out Of Scope / Future

- The "graph" direction is the agent building — and reusing — **its own** task
  structure (grown from the per-turn scratchpad toward self-authored, reusable
  task graphs). It is **not** a LangGraph-style control-flow engine over fixed
  stages; do not build one.
- **Skills** (declarative SKILL.md registry) and a sandboxed execution substrate
  (forkd) are planned, not present.
- Packaging: a `contrib/` folder and a `.deb` are future work.
