# Structure — the code map

Albert is a single binary crate. Modules by concern (all under `src/`):

| Module | Owns |
|--------|------|
| `main.rs` | Wiring: load `.env` + `albert.toml`, open the kaeru vault, build history (memory / file / sqlite), the scheduler, the scratchpad, the skill catalog, the prompt loader; export `$OCTO_CODE_WORKSPACE` + `$OCTO_SKILLS_DIR`; register the connector factories (telegram, caldav, scheduler, storage, forkd) and `from_config_file`, or fall back to console; run the Octo runtime. |
| `cogitator.rs` | `AlbertCogitator` — the `Cogitator`: the perceive → reflex → assemble-context → tool-loop → reply cycle for `chat.message` and `alarm.fired`. Holds config, kaeru handle, history, scratchpad, skills, prompt loader; builds the rig agent + the full toolset (dispatch, kaeru verbs, scratchpad, file workspace, `send_file`, skills, and — owner-only — `restart`). |
| `acl.rs` | The owner-only Telegram ACL admin reflex (`/allow` `/deny` `/allowed`): role check + dispatch of the `octo.telegram.*` control command. `is_owner` (role == owner) also gates the owner-only `restart` tool. Deterministic — no LLM in a security action. |
| `skills.rs` | The declarative skill system: a folder of `<name>/SKILL.md` (frontmatter `name`/`description` + body). Renders the per-turn catalog (full when small, a count + `skill_search` pointer when large — threshold `[skills] page`); the `skill_list` (paginated) / `skill_search` (ranked by name/description) / `skill_apply` / `skill_file` tools; an LRU read-through cache so only applied bodies (not the whole folder) sit in context. |
| `routines.rs` | Proactive routines: idempotently seed the base memory-reflection alarm on startup (retry until the scheduler answers). The routine handler itself lives in the cogitator (`run_routine`). |
| `scratchpad.rs` | The loop scratchpad: a channel-keyed in-memory store + the five rig tools (`scratchpad_goal/step/mark/note/clear`) and the render shown each turn. |
| `status.rs` | Live progress feedback: a rig `PromptHook` (`StatusFeed`) that streams the agent's tool calls / thoughts into one in-place-edited chat status message (opt-in `stream_status`), plus the always-on typing indicator. |
| `prompt.rs` | `PromptFiles` — loads `soul.md` + `system.md` into RAM and hot-reloads them by mtime; terse embedded fallback if a file is missing. |
| `config.rs` | `Config` — parse `albert.toml` (serde + `toml`), resolve the LLM secret from env, resolve relative paths against the config dir, supply defaults (auth mode, history backend, skills, workspace, timezone, clouds). |
| `history.rs` | Bridge to Octo's `octo-history`: the per-channel transcript (the hot-context tier), plus conversion of stored turns into rig messages. Backend selected by `[history] backend` — in-memory, JSON file, or migrated SQLite. |
| `console.rs` | `ConsoleConnector` — a stdin/stdout channel, the Telegram stand-in for local dev. |
| `codex_http.rs`, `codex_model.rs` | ChatGPT-subscription path: a custom rig `HttpClientExt` that rewrites requests/responses for the OpenAI **Codex** backend (streaming-only, no `system` messages / strict schemas), and the Codex model slugs. Only in play under `auth = "subscription"`. |
| `openai_auth.rs`, `openai_login.rs` | Subscription OAuth: the PKCE sign-in (`albert login`) and in-place access-token refresh; reads/writes `auth.json`. |
| `error.rs` | The crate error type — explicit `#[from]` variants, no `anyhow`. |

## Where a concern lives

- **The action space** (what the model can do) = the registered connectors' catalogs,
  assembled in `cogitator.rs::catalog` from `ctx.connectors()`. Add an organ → it
  appears automatically in the `dispatch_to_connector` tool (scheduler, calendar,
  storage, forkd all reached this way).
- **The memory tools** = kaeru verbs, added in `cogitator.rs::run_agent` from the
  `KaeruMemory` handle (plus the cloud verbs when a `[clouds.*]` is configured).
- **Files & the workspace** = the octo-code tools (`read/write/edit/list/glob/grep`,
  jailed to `$OCTO_CODE_WORKSPACE`) and `send_file`, added in `run_agent`; the
  workspace is one shared directory that storage + forkd also inherit by name.
- **Skills** = `skills.rs` (catalog + the `skill_*` tools); the folder is `[skills] dir`.
- **Scripts** = the `forkd` connector, reached over `dispatch_to_connector`
  (`forkd.run`); executable skills name a bundled script run in place via `skill_path`.
- **Self-restart** = the owner-only `restart` tool (`octo-rig`), added in `run_agent`
  only when `is_owner`; it emits the `octo.control.*` signals the runtime carries out.
- **A user reminder vs. a routine** = distinguished by the `alarm.fired` payload in
  `cogitator.rs::on_alarm` (`routine` marker → silent `run_routine`; else the
  reminder path).
- **Connector config** (Telegram, calendar, storage, forkd) = the manifests under
  `config/`, not the code — see [configuration.md](configuration.md).

## House style

- Files stay under ~500 lines (split a module when it grows past it).
- Imports are brace-grouped per crate; the leaf entity is imported so code bodies
  carry no module path (`Duration::from_secs`, not `std::time::Duration::from_secs`).
- No emoji in code or logs.
- Conventional commit prefixes: `feature:` / `fix:` / `chore:` / `docs:` / `git:`.

## Dependencies

Octo (bus, connectors, supervision, the rig bridge + file tools) and kaeru (memory)
are pulled as **git dependencies** (see `Cargo.toml`) — one repo each, many crates,
pinned to a rev. There is no local sibling-checkout requirement.
