# Structure — the code map

Albert is a single binary crate. Modules by concern (all under `src/`):

| Module | Owns |
|--------|------|
| `main.rs` | Wiring: load `.env` + `albert.toml`, open the kaeru vault, build history, the scheduler, the scratchpad, the prompt loader; register the connector factories and `from_config_file`, or fall back to console; run the Octo runtime. |
| `cogitator.rs` | `AlbertCogitator` — the `Cogitator`: the perceive → reflex → assemble-context → tool-loop → reply cycle for `chat.message` and `alarm.fired`. Holds the config, kaeru handle, history, scratchpad, and prompt loader; builds the rig agent + toolset. |
| `acl.rs` | The owner-only Telegram ACL admin reflex (`/allow` `/deny` `/allowed`): role check + dispatch of the `octo.telegram.*` control command. Deterministic — no LLM in a security action. |
| `routines.rs` | Proactive routines: idempotently seed the base memory-reflection alarm on startup (retry until the scheduler answers). The routine handler itself lives in the cogitator (`run_routine`). |
| `scratchpad.rs` | The loop scratchpad: a channel-keyed in-memory store + the five rig tools (`scratchpad_goal/step/mark/note/clear`) and the render shown each turn. |
| `prompt.rs` | `PromptFiles` — loads `soul.md` + `system.md` into RAM and hot-reloads them by mtime; terse embedded fallback if a file is missing. |
| `config.rs` | `Config` — parse `albert.toml` (serde + `toml`), resolve the LLM secret from env, resolve relative paths against the config dir, supply defaults. |
| `history.rs` | Bridge to Octo's `octo-history`: the per-channel transcript (the hot-context tier), plus conversion of stored turns into rig messages. |
| `console.rs` | `ConsoleConnector` — a stdin/stdout channel, the Telegram stand-in for local dev. |
| `error.rs` | The crate error type — explicit `#[from]` variants, no `anyhow`. |

## Where a concern lives

- **The action space** (what the model can do) = the registered connectors' catalogs,
  assembled in `cogitator.rs::catalog` from `ctx.connectors()`. Add an organ → it
  appears automatically.
- **The memory tools** = kaeru verbs, added in `cogitator.rs::run_agent` from the
  `KaeruMemory` handle.
- **A user reminder vs. a routine** = distinguished by the `alarm.fired` payload in
  `cogitator.rs::on_alarm` (`routine` marker → silent `run_routine`; else the
  reminder path).
- **Connector config** (Telegram, calendar) = the manifests under `config/`, not the
  code — see [configuration.md](configuration.md).

## House style

- Files stay under ~500 lines (split a module when it grows past it).
- Imports are brace-grouped per crate; the leaf entity is imported so code bodies
  carry no module path (`Duration::from_secs`, not `std::time::Duration::from_secs`).
- No emoji in code or logs.
- Conventional commit prefixes: `feature:` / `fix:` / `chore:` / `docs:` / `git:`.

## Dependencies

Octo (bus, connectors, supervision) and kaeru (memory) are pulled as **git
dependencies** (see `Cargo.toml`) — one repo each, many crates. There is no local
sibling-checkout requirement.
