# Structure — the code map

Albert is a Cargo workspace producing a single `albert` executable. Browser
projects live under `web/`; reusable Rust code and transport adapters have
separate crate boundaries.

```text
Cargo.toml                 virtual workspace, shared dependencies and profiles
Cargo.lock
albert-core/               cognition and transport-neutral domain library
connectors/
  console/                 stdin/stdout adapter
  telegram/                Albert policies over Octo Telegram
  web/                     HTTP API, panel services and embedded assets
albert-server/             server binary, CLI and runtime assembly
web/
  albert-panel/            React + TypeScript browser project
  ui-dist/                 generated embedded bundle (ignored)
xtask/                     schema export tooling
config/                    connector manifests and examples
skills/                    bundled skill packages
docs/                      public architecture and configuration guides
```

## Crate responsibilities

| Package | Owns |
|---------|------|
| `albert-core` | `AlbertCogitator`, context and provider adapters, runtime lifecycle, memory, sessions, messaging, scratchpad, subagents, skills, neutral status events and internal persistence. |
| `albert-connector-console` | Console input/output and its session/approval integration. |
| `albert-connector-telegram` | Trusted native ingress mapping, Telegram host policy, session navigation, feedback and delivery adaptation. The transport itself remains in Octo. |
| `albert-connector-web` | HTTP DTOs/router, JWT/CSRF, accounts and owner configuration services, OAuth panel flow, private-session bridge, SSE, assets and plain Swagger. |
| `albert-server` (binary `albert`) | Process environment, interactive CLI login, composed settings, bootstrap, connector factories, SQL protected paths and runtime assembly. |
| `xtask` | Export the same Utoipa schema used by the running API without requiring browser assets. |

The application depends on core and the connectors; each connector depends on
core. Core has no dependency on Albert's application or connector crates. HTTP
DTOs and Telegram wire types stay at the edge. Domain ingress, feedback, delivery
and status contracts let the same session work across transports without importing
their implementations into cognition. Adapter integration tests belong outside
core; domain tests use neutral fixtures.

Generic SQL, scheduler, browser, mail, storage and other connectors are consumed
from Octo. They are assembled by the application, not copied into local crates.
Noninteractive OAuth/PKCE belongs to core; terminal input, browser launching and
the loopback callback belong to the application.

## Persistence and build outputs

Core owns the internal database and the single Albert migration ledger. The web
adapter uses that same pool for account/configuration services. Conversation
catalogs and canonical journals remain in octo-history; application SQL stores
remain isolated from internal history. Moving code does not change schema versions,
migration checksums, configuration discovery or deployed data paths.

Utoipa defines the API schema. `xtask` exports it for TypeScript generation; no
OpenAPI JSON snapshot is committed. Generated TypeScript declarations may be
versioned and are checked for freshness. The browser build produces disposable
assets, and the web crate verifies their fingerprints and copies them to Cargo's
`OUT_DIR` for embedding. Cargo never installs Node packages. Core and schema-only
builds do not require a browser bundle; a production build requires current assets.
See [web-panel.md](web-panel.md#frontend-development-and-embedding) for commands.

## Where a concern lives

- **The action space** (what the model can do) = the registered connectors' catalogs,
  assembled in `albert-core/src/cogitator/context.rs::catalog` from `ctx.connectors()`. Add an organ → it
  appears automatically in the `dispatch_to_connector` tool (scheduler, calendar,
  storage, forkd all reached this way).
- **The memory tools** = kaeru verbs installed in `albert-core/src/cogitator/agent.rs::drive` through
  `Memory::install`: embedded `KaeruMemory`, or the dedicated MCP memory session.
  Embedded cloud verbs are enabled by `[clouds.*]`; MCP cloud settings belong to
  the memory server.
- **Files & the workspace** = the octo-code tools (`read/write/edit/list/glob/grep`,
  jailed to `$OCTO_CODE_WORKSPACE`) and `send_file`, added in `run_agent`; the
  workspace is one shared directory that storage + forkd also inherit by name.
- **Skills** = `albert-core/src/skills/` (catalog + the `skill_*` tools); the folder is `[skills] dir`.
- **Scripts** = the `forkd` connector, reached over `dispatch_to_connector`
  (`forkd.run`); executable skills name a bundled script run in place via `skill_path`.
- **Self-restart** = the owner-only `restart` tool (`octo-rig`), added in `run_agent`
  only when `is_owner`; it emits the `octo.control.*` signals the runtime carries out.
- **A user reminder vs. a routine** = distinguished by the `alarm.fired` payload in
  `albert-core/src/cogitator/alarms.rs::on_alarm` (`routine` marker → silent `run_routine`; else the
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
