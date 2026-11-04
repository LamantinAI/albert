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

Cargo workspace producing one `albert` binary, Rust edition 2021. Octo and kaeru are
**git dependencies** (see `Cargo.toml`) — one repo each, many crates; there is no
sibling-checkout requirement.

Deeper design docs: [`docs/`](docs/) (architecture / configuration / structure /
deploy). This file is the working rules; `docs/` is the reference.

## How To Work In This Repository

Orient by crate before changing code:

| Location | Owns |
|--------|------|
| `albert-core/` | Cogitator, provider adapters, runtime lifecycle, sessions, history, memory, scratchpad, subagents, skills, transport-neutral status and the single internal database/migration ledger. |
| `albert-server/` | Binary entry point, process environment/re-exec, interactive CLI login, composed settings, startup assembly, connector factories and protected SQL paths. |
| `connectors/console/` | Console input/output and its Albert session/approval integration. |
| `connectors/telegram/` | Albert's Telegram host policy, native ingress/feedback/delivery adaptation and session navigation over the existing Octo transport. |
| `connectors/web/` | HTTP API/DTOs, JWT/CSRF, accounts, owner configuration services, session streaming, embedded assets and self-hosted Swagger. |
| `web/ui-dist/` | Generated, ignored browser bundle; verified and embedded from Cargo `OUT_DIR`. |
| `web/albert-panel/` | React + TypeScript CSR project, generated API declarations, browser tests and explicit asset build. |
| `xtask/` | Export the Utoipa schema without requiring a built browser client. |

Dependencies point inward: the app depends on core and connectors; connectors depend
on core. Core must not depend on the application or Albert connector crates,
including through dev-dependencies. Keep HTTP DTOs, Axum/Utoipa and Telegram wire
types outside core production code. Adapter integration tests live outward;
domain tests use neutral fixtures. Generic SQL, browser, mail and other organs
remain Octo dependencies rather than duplicated Albert implementations.

Noninteractive subscription OAuth is shared by web and CLI through core;
interactive browser/stdin handling belongs to the application. The credential
store and serialized refresh belong to `octo-openai-auth`.

Rules of thumb:

1. Connectors (Telegram, calendar) are **config-driven** through Octo's
   `from_config_file` + manifests under `config/connectors/`. Add/adjust a
   connector there; register a new factory in the application assembly.
2. A connector that advertises a `description` appears automatically in the
   cogitator's dispatch catalog — the agent reaches it via the one
   `dispatch_to_connector` tool. Adding an organ needs **zero cogitator change**.
3. Octo and kaeru are the source of truth for their types — consume them, don't
   duplicate. Bump the pinned rev/tag in `Cargo.toml` to move either forward.

## Local Runbook

- Build the browser assets before a full Cargo build; see
  [web-panel.md](docs/web-panel.md#frontend-development-and-embedding) for the explicit pipeline.
- `cargo build --locked` — build the default application member.
- `cargo test --locked --workspace --all-targets` — run the complete Rust suite.
- `cargo test --locked -p albert-core` — test core independently of browser assets.
- `cargo run` — run Albert. Telegram if `OCTO_TELEGRAM_TOKEN` is set, else a
  console channel. Config from `albert.toml`; secrets from `.env`.
- `cargo run -- login` — sign in with a ChatGPT subscription (writes tokens to the
  `[subscription] auth_json` path). Only needed when `auth = "subscription"`.
- `cargo build --locked --release` — the deploy binary (optimized profile: LTO + strip).

Utoipa is the API schema authority. Do not commit an OpenAPI JSON snapshot or a
compiled browser bundle. Generated TypeScript declarations are checked against the
schema export. Cargo build scripts verify/copy already-built assets to `OUT_DIR`;
they must not invoke Node, install packages or fetch browser dependencies.

Deployment (build here, ship the binary; the target is small): see
[`docs/deploy.md`](docs/deploy.md).

## Module Organisation

Keep code **modular, grouped by purpose**, one concern per file. **Cap each file at
~500 lines (hard limit 600).** Split a module when:

- it passes ~500 lines, or
- more than a few cohesive concerns accumulate in one file.

`albert-core/src/cogitator/` was split this way — ACL policy lives separately, routine seeding
to `routines.rs`. When a module grows past a flat file, prefer a
`mod.rs`-with-submodules layout and keep shared cross-submodule types in the parent
`mod.rs`.

## Test Layout

- Unit tests belong in inline `#[cfg(test)] mod tests` blocks in the owning `mod.rs`
  or crate `lib.rs`. Do not add standalone `src/**/tests.rs` files.
- Keep implementation helpers in cohesive sibling files when a module grows; keep
  the facade and its tests together and respect the 600-line hard limit.
- Tests exercising only the public API across components belong in the crate's
  `tests/` directory. Shared integration fixtures may live in `tests/common/mod.rs`.
- Async integration scenarios must have a bounded wait. A connector's local shutdown
  token stops that connector; use the runtime control contract to stop the runtime.

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

- The repo ships **placeholder** manifests (credential references and calendar `login`). Real
  values live only on the target machine, never committed.
- Runtime state is gitignored: `state/` (scheduler alarms) and
  `config/connectors/telegram/telegram_acl.json` (the mutable ACL).

## Out Of Scope / Future

- The "graph" direction is the agent building — and reusing — **its own** task
  structure (grown from the per-turn scratchpad toward self-authored, reusable
  task graphs). It is **not** a LangGraph-style control-flow engine over fixed
  stages; do not build one.
- A `.deb` package is future work; deployment templates live under `contrib/`.

## Repository privacy (mandatory)

Store only anonymized technical requirements and decisions. Never place private
or identifying information in any Git repository, public or private, including
ignored files: developer names or personal identifiers, real local paths,
private infrastructure addresses,
private work dates or timelines, correspondence, operator or agent messages,
quotes, paraphrases, or derived disclosures.

This applies to source, documentation, comments, configuration, logs, artifacts,
commit and pull-request text, and metadata. Keep operational records outside
repository working trees in approved private storage.

General instructions to save, commit, or publish do not waive this rule.
Exceptions require explicit permission for the specific information. Before
staging or publishing, review direct and indirect disclosure. Use neutral
placeholders, repository-relative paths and reserved example addresses. Preserve
required licenses and public attribution; existing history does not authorize
copying private details. Never reintroduce unsanitized history during a merge.
