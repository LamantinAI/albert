# Configuration

Albert is config-as-data: nothing tunable is hardcoded. There are three layers —
**secrets** (`.env`), **Albert-level config** (`albert.toml`), and **connector
manifests** (`config/`). Secrets only ever live in the environment; every config file
is safe to commit (it names secrets by their env-var name, never the value).

This page is a configuration reference. See [A working config](#a-working-config)
for the starting files for standalone and Compose installations.

## `.env` — secrets only

Put `.env` in the deployment directory beside `albert.toml`. Albert looks there
for saved secret values and falls back to the process environment. Set
`ALBERT_CONFIG` in the process environment before starting Albert to select a
different config file.

```sh
ALBERT_OPENAI_KEY=sk-or-...            # LLM API key (named by openai_key_env in albert.toml)
OCTO_TELEGRAM_TOKEN=123456:ABC...      # Telegram bot token (named by token_env; omit -> console)
# --- calendar: basic auth (Yandex/Fastmail/Nextcloud/iCloud) ---
OCTO_YANDEX_APP_PASSWORD=...           # an APP password (named by password_env in the manifest)
# --- or calendar: Google OAuth2 (named in the manifest) ---
# GOOGLE_CLIENT_SECRET=...
# GOOGLE_REFRESH_TOKEN=...
# --- mail: only if you enable the (off-by-default) mail connector ---
# OCTO_MAIL_USER=...                   # IMAP/SMTP login  (named in mail.toml)
# OCTO_MAIL_PASS=...                   # IMAP/SMTP app-password (named in mail.toml)
```

kaeru reads its own `KAERU_*` env (`KAERU_VAULT_PATH`, etc.) directly.

## `albert.toml` — Albert-level config

Albert looks for `ALBERT_CONFIG` first, then `albert.toml` next to the binary,
then `albert.toml` in the working directory. Relative paths inside it resolve
against the file's own directory.

```toml
auth  = "api_key"                        # "api_key" (default) | "subscription"
model = "deepseek/deepseek-v4-flash"
base_url = "https://openrouter.ai/api/v1"
openai_key_env = "ALBERT_OPENAI_KEY"     # the env var holding the LLM key

# multimodal    = true   # can the model see photos? omit -> heuristic:
                         # subscription = yes; api_key = guessed from the model name
# stream_status = true   # stream tool calls / thoughts into the chat while a
                         # turn runs (one in-place edited status message)
# Hearing (voice messages in chat), speaking and drawing are connectors on the
# subscription token: see "Subscription organs" below.

# Owner's timezone (IANA name). The agent's "current time" — and any reminder times
# it computes — render in this zone, not UTC. Match the calendar connector's own
# `timezone` so listed events read local. Defaults to UTC.
timezone = "Europe/Moscow"

[prompt]
soul   = "soul.md"       # persona/tone           (hot-reloaded)
system = "system.md"     # operating instructions (hot-reloaded)

[reflection]
period_secs = 21600      # base memory-reflection routine cadence (6h)

[history]
backend = "sqlite:state/history.db"   # retained session journals
backup_dir = "state/history-backups" # pre-upgrade SQLite snapshots
attachment_root = "state/attachments" # retained files, keyed by session and asset ID

[scheduler]
state_path = "state/scheduler.json"   # where alarms persist (survives restart)

[agent]
max_tool_turns = 30      # max rig tool-loop rounds per message

[skills]
dir   = "skills"         # folder of <name>/SKILL.md (catalog shown each turn)
cache = 5                # LRU size for applied skill bodies
page  = 10               # skill_list page size only (see below)

[code]
workspace = "state/workspace"   # ephemeral file-tool jail ($OCTO_CODE_WORKSPACE)
```

Retained chat attachments use `[history].attachment_root`. The path is relative
to the configuration file unless absolute. Keep this directory with SQLite in
backups and preserve its `<session-id>/<attachment-id>` layout when moving it.
Upgrading from SQLite attachments copies and verifies the payloads before the
schema removes the old blobs. Temporary chat attachments stay in RAM until the
chat is saved.


### Auth: API key vs. ChatGPT subscription

`auth` selects how the LLM call authenticates:

- `auth = "api_key"` (**default**) — an API key against `base_url` (OpenRouter or any
  OpenAI-compatible endpoint). The key lives in the env var named by
  `openai_key_env` (default `ALBERT_OPENAI_KEY`). This is the default path.
- `auth = "subscription"` — a **ChatGPT subscription** (Plus/Pro/Team/…): model
  calls go to the OpenAI **Codex backend** using OAuth tokens instead of a metered
  API key. An option, not the default. No `openai_key_env` is needed.

```toml
model = "gpt-5.4"          # a Codex-backend slug: gpt-5.6-sol, gpt-5.5, gpt-5.4, gpt-5.4-mini, …
auth  = "subscription"

[subscription]
auth_json = "~/.codex/auth.json"                    # where the OAuth tokens live (default: reuse the codex CLI login)
base_url  = "https://chatgpt.com/backend-api/codex" # the Codex backend (default)
```

Get the tokens either way:

- **`albert login`** — prints the OpenAI verification address and a one-time
  device code. Sign in and enter the code on that page within 15 minutes.
  Albert stores the tokens at `auth_json` after approval. Enable device-code
  sign-in in your ChatGPT security settings or through your workspace admin.
- **`codex login`** — since `auth_json` defaults to `~/.codex/auth.json`, an existing
  codex CLI login is reused as-is.

Either way, the access token becomes the `Authorization: Bearer`, the account id the
mandatory `ChatGPT-Account-ID` header. Albert **refreshes the access token in place**
before it expires (via the OAuth refresh token), so a session started days later just
works; only when the refresh token itself lapses do you re-run `albert login`. The
Codex `/responses` endpoint is streaming-only and rejects `system` messages / strict
schemas — Albert reconciles all of that transparently (see [`albert-core/src/codex_http/mod.rs`]).

[`albert-core/src/codex_http/mod.rs`]: ../albert-core/src/codex_http/mod.rs

> Note: driving a ChatGPT subscription from a third-party app uses OpenAI's Codex
> client credentials outside their officially-supported path — use it with your own
> subscription, at your own discretion.

### History backend

`[history] backend = "sqlite:state/history.db"` is the default and the supported
Albert Session backend. It retains complete journals and independent compact
checkpoints. The path and `backup_dir` resolve against the directory containing
`albert.toml`; absolute paths remain absolute. Memory/file history backends remain
available in Octo for other assemblies, but Albert refuses them explicitly at
Session startup instead of silently replacing their data.

Before schema upgrades or actual legacy channel data migration, startup creates
a consistent `VACUUM INTO` snapshot, including
committed WAL data, under `backup_dir` (default `state/history-backups`). On Unix,
the directory is mode 700 and completed snapshots are mode 600. A backup failure
stops startup before either migration owner opens the database. Both the actual
history database and backup directory are protected from user SQL stores.

See [sessions.md](sessions.md) for private/group access, temporary chats, title
settings, limits, identity links and automatic legacy import. No browser login is
enabled by this Session foundation.

`[sessions.messaging]` controls host-only private session requests, inspection
byte budgets, report delivery and retention. See [session-messaging.md](session-messaging.md)
for the model tool, defaults and transport outcome semantics.

### Prompt files (`soul.md`, `system.md`)

`soul.md` is the persona; `system.md` the operating protocol (memory, reminders,
scratchpad, files, skills, scripts, restart). The system prompt is deliberately
**compact** — capabilities that would bloat it (a skill's full instructions, a
memory) are pulled on demand as tools, not pre-loaded. Both files are loaded into RAM
once and re-read only when their **mtime** changes — edit and save, no restart, and no
re-reading big files on every request. A missing file falls back to a terse embedded
default.

### Skills (`[skills]`) and the file workspace (`[code]`)

- `[skills] dir` is a folder of `<name>/SKILL.md` (frontmatter `name`/`description` +
  body, optionally bundling resource files). Only the **catalog** (name + when-to-use)
  is shown to the model each turn; `skill_apply` loads a body on demand and keeps it in
  an LRU cache of `[skills] cache` entries. A missing folder just means no skills.
  - **Discovery:** every available skill's name and full when-to-use description
    stays in the preamble, regardless of the number of installed skills. Bodies
    still load only through `skill_apply`, except for `always: true` standing
    instructions. `[skills] page` (default 10) controls only `skill_list` pagination;
    `skill_search <keywords>` remains available to search names and descriptions.
    Catalog metadata grows with the number of skills; adding a skill never hides
    the existing ones or truncates their trigger descriptions.
  - **Commands:** a skill can declare a chat command in its frontmatter —
    `command: brief` (lower-case letters, digits, `_`, `-`; a channel's stricter rules —
    Telegram's 1-32 of `a-z0-9_` — are applied by that channel to its menu), `command_about: Your day at a glance` (its one
    line in `/help` and the bot menu; omitted → cut from `description`), and
    `command_owner: true` to keep it to the owner. `/brief args` then starts an ordinary
    agent turn seeded with that skill's instructions and the arguments. System commands
    (`/help`, `/start`, `/cancel`, `/restart`, `/model`, `/chatinfo` and session controls)
    are reserved and built in: they answer at once, without the model — a skill claiming
    one of those names (or a name another skill already took) is logged and not
    registered. `/help` lists the commands permitted for the current account. Telegram
    publishes native command menus by verified account role; configuration and
    restart controls remain owner-only.
- `[code] workspace` is the ephemeral scratch directory the file tools
  (`read/write/edit/list/glob/grep`) are **jailed** to, exported as
  `$OCTO_CODE_WORKSPACE`. The forkd runner (cwd) and the storage connector
  (promote/checkout) inherit the **same** directory by name, so they operate on the
  files the agent wrote with zero path coordination. octo-code creates it on first use.

### Memory backend (`[memory]`)

Memory has one selected backend, independent of the connector catalog. Omit this
section to keep embedded kaeru, or explicitly set `backend = "embedded"`.
Embedded kaeru uses `KAERU_VAULT_PATH` as before. A dedicated **kaeru MCP server**
can instead own the vault:

The web panel exposes this choice in **Settings → Agent → Memory**. On a fresh
browser-first installation it also appears immediately after the first model
connection, for both API keys and subscriptions, when no backend has been
selected. Existing installations and fresh deployments with a valid explicit
`[memory]` backend, including production Compose, skip this step. The internal
setup checkpoint records the selection while the selected Kaeru storage holds
the memory contents.

HTTP bearer tokens can be entered through the masked field, or referenced by an
existing credential name. Values go to the server secret store and are never
returned by the memory API. A blank token field keeps the saved credential.
Changing a running agent's backend or credentials saves the desired settings and
shows **Restart required**; an explicit process restart applies them. An unchanged
selection, or a change reverted to the active settings, needs no restart. Switching
backends preserves the original storage and does not copy its contents.

```toml
[memory]
backend = "mcp"
transport = "http"
url = "http://127.0.0.1:9876/mcp"
timeout_secs = 30
# token_env = "ALBERT_MEMORY_TOKEN"
```

HTTP uses MCP Streamable HTTP (JSON and SSE responses), with normal TLS
certificate verification and an optional bearer token read from the named
environment variable. The URL is the **MCP endpoint**, not a kaeru-cloud REST
endpoint. Remote HTTPS servers can use the same settings. A named but unset or
empty token variable is an error; credentials do not belong in the URL.

For kaeru's local stdio bridge (omit `env` for a daemon without authentication):

```toml
[memory]
backend = "mcp"
transport = "stdio"
command = "kaeru-mcp"
args = ["--stdio"]
timeout_secs = 30
env = { KAERU_MCP_AUTH_TOKEN = "ALBERT_MEMORY_TOKEN" }
```

`env` maps each child variable to a **parent environment variable name**, not its
value. Bare commands resolve through `PATH`; relative command paths resolve from
the config directory. Arguments are passed directly, without a shell. The child
inherits only `PATH`, `HOME`, `LANG`, `TMPDIR`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`
and the explicit mapping. `kaeru-mcp --stdio` forwards to the local daemon,
starting it if necessary; the daemon owns the vault. For a separate daemon, map
`KAERU_MCP_LISTEN_PORT` and `KAERU_VAULT_PATH` to dedicated parent variables too.
Changing the bridge's vault variable does not reconfigure an already running
daemon on the same port. Install `kaeru-mcp` separately for stdio; the Albert
Docker image does not bundle it. Do not point embedded Albert and a separate MCP
daemon at the same RocksDB vault concurrently.

Albert connects, discovers all pages of tools and runs its **v1 application
migration before accepting events**. It checks `initiatives`; if `albert` is
absent, it makes an ordinary `note` of a short Albert identity observation with
`initiative = "albert"`, then verifies creation. This uses kaeru's existing
capture path: writing the record also registers its initiative. Subsequent
starts leave an existing initiative unchanged. Embedded memory makes the same
call through `kaeru-rig`; no separate schema-marker record is created.
Kaeru itself owns its database schema migrations. No local conversations or
memories are copied when switching backends.

MCP verbs appear as `kaeru_<verb>`, with the server's schemas. Initiative-scoped
calls default to `albert`; an explicit project name is preserved, just as with
embedded memory. Search uses `initiatives = ["albert"]`; context graph selection
and explicitly global exports keep their own scope. The automatic reflection
routine uses the selected backend. Use a **Kaeru 0.8.0-compatible MCP server**;
Albert pins its embedded dependency to **0.8.0**. Required native tools include
`initiatives`, `note`, `context`, `search`, `read`, `task`, `task_update`,
`reference` and `reflect`. Custom instructions should use those canonical tools
and their current argument schemas.

Startup checks the typed response state, operation and API version. Partial or
incomplete initiative discovery is not evidence that an initiative is absent.
Read-only temporary conversations and restricted children receive only tools
classified as read-only by the SDK; `context` is excluded because it can update
reminder delivery metadata.

`timeout_secs` bounds connection, discovery and each call. A failed startup or
migration prevents agent activation; the web panel keeps the saved choice and
offers **Retry initialization**. There is no silent fallback to local memory.
A transport failure is returned to the agent, and the next call reconnects
and checks the initiative again. The failed call is not automatically replayed:
a write whose response was lost may already have committed. Changed tool schemas
after a reconnect require restarting Albert. Changing `[memory]` also requires a
process restart.

With MCP, configure kaeru-cloud endpoints **on the MCP server**. Albert rejects
`[clouds.*]` together with MCP memory to avoid silently ignoring those settings.

### Cloud memory (`[clouds.*]`, embedded backend)

Optional. By default Albert's memory (kaeru) is **local-only** and the
share/pull/cloud_recall tools are not even shown to the model. Declare one or more
`kaeru-cloud` endpoints and Albert can share to / pull from team knowledge:

```toml
[clouds]
default = "family"        # optional; the cloud used when a tool's `cloud` arg is omitted

[clouds.family]
url       = "https://cloud.family.example"
token_env = "ALBERT_CLOUD_FAMILY_TOKEN"   # the env-var NAME; the value lives in .env

[clouds.work]
url       = "https://kaeru.work.example"
token_env = "ALBERT_CLOUD_WORK_TOKEN"
```

As with every secret, the config names the token's **env var**, never its value.
With at least one cloud declared Albert installs the cloud tools (`kaeru_share`,
`kaeru_pull`, `kaeru_cloud_recall`, `kaeru_policy`, `kaeru_link_cloud`,
`kaeru_cloud_links`, `kaeru_sync_review`); with none it stays local-only.

## Connector manifests — `config/`

Connectors are config-driven through Octo's `from_config_file`: `main.rs` registers
each type's factory, then points the builder at `config/octo.toml`, which scans a
`connectors/` directory of per-connector manifests.

```
config/
├── octo.toml                          # [connectors] dir = "connectors"
└── connectors/
    ├── telegram/telegram.toml         # account-authenticated chat channel
    ├── calendar/calendar.toml         # CalDAV calendar
    ├── storage/storage.toml           # durable object store
    ├── forkd/forkd.toml               # sandboxed script runner
    ├── search/search.toml             # web search (DuckDuckGo; links the system libcurl)
    ├── jira/jira.toml                 # Jira via the generic http connector (placeholder base_url)
    └── mail/mail.toml.example         # IMAP/SMTP organ — off by default (.example, not loaded)
```

This path runs when a Telegram token is present; without one Albert uses a console
channel for local dev.

### `connectors/telegram/telegram.toml`

Configure Telegram by selecting the Telegram instance in **Settings → Connections**. The owner saves the BotFather
credential and connects the bot. Each panel user creates a personal, single-use
login token in the same connector settings and sends `/login <token>` in the
bot's private chat. Permissions come from that user's current panel account.

```toml
[connector]
id = "telegram"
type = "telegram"
token_env = "OCTO_TELEGRAM_TOKEN" # credential value stays outside the manifest
address_names = ["Albert"]
```

The panel stores a write-only, connector-specific credential reference. `owner_chat`,
`admins` and legacy allowlists no longer authorize Albert users. Existing journals
and the selected private conversation survive the initial required login; later
restarts retain the binding. No manual history mapping is required.

| Command | Effect | Who |
|---------|--------|-----|
| `/login <token>` | Link this Telegram identity to a panel account | Private chat only |
| `/logout` | Revoke this Telegram binding; retain saved conversations | Private chat only |
| `/chatinfo` | Show message source information | Registered users |
| `/sessions`, `/new`, `/open`, `/rename`, `/tmp`, `/save`, `/end` | Manage personal conversations | Registered users, private chat only |
| `/cancel`, `/compact` | Control the current conversation | Registered users in their private chat; owner in groups |
| `/model`, `/restart` | Change the model or restart Albert | Owner |

Unregistered private chats expose login only. In groups, anyone may explicitly
address Albert by @mention, a reply or a configured name. Unregistered guests
receive conversational answers without tools; commands require a linked account.
Unaddressed chatter stays quiet. A guest or another registered user cannot
interrupt an owner's active task. The group's history stays bound to its channel,
without access to personal session navigation. Internal tool traces and private
execution context are excluded from the shared group prompt.

Telegram must deliver group updates to the bot: configure BotFather group/privacy
access or grant the bot the required group access. `/command@another_bot` is ignored.

See [Telegram account access](telegram-accounts.md) for token storage and revocation.

### `connectors/calendar/calendar.toml`

Two auth modes. **Basic (app password)** — the simple path, discovers its own
collection (Yandex/Fastmail/Nextcloud/iCloud):

```toml
[connector]
id       = "calendar"
type     = "caldav"
timezone = "Europe/Moscow"              # render event start/end in this zone (default UTC)
reminder_minutes = 10                   # default popup lead time for events Albert creates

base_url     = "https://caldav.yandex.ru"   # the collection is auto-discovered
auth         = "basic"
login        = "you@yandex.ru"
password_env = "OCTO_YANDEX_APP_PASSWORD"    # APP password in the env, NOT the account password
```

**Google (OAuth2)** — needs the full `calendar` scope, the CalDAV API enabled, and an
**explicit** collection (Google 404s the discovery chain):

```toml
[connector]
id       = "calendar"
type     = "caldav"
timezone = "Europe/Moscow"
reminder_minutes = 10

collection        = "https://apidata.googleusercontent.com/caldav/v2/you@gmail.com/events"
auth              = "oauth2"
token_url         = "https://oauth2.googleapis.com/token"
client_id         = "<oauth-client-id>.apps.googleusercontent.com"
client_secret_env = "GOOGLE_CLIENT_SECRET"
refresh_token_env = "GOOGLE_REFRESH_TOKEN"
```

Commands are the same either way: `calendar.{list,create,delete}_event`. Two calendars
later = a second manifest with a different `id` (e.g. `calendar-work` /
`calendar-personal`); same `type = "caldav"`.

### `connectors/storage/storage.toml`

```toml
[connector]
id      = "storage"
type    = "storage"
backend = "local"
# `root` is durable + backed up (NOT the throwaway workspace), relative to THIS
# manifest's directory — climb to the deploy root, land in state/ beside the workspace.
root    = "../../../state/artifacts"
# `workspace` omitted on purpose -> promote/checkout use $OCTO_CODE_WORKSPACE,
# the same jail octo-code writes to, so the two stay in lockstep.
```

### `connectors/forkd/forkd.toml`

```toml
[connector]
id   = "forkd"
type = "forkd"
# `workspace` omitted -> inherits $OCTO_CODE_WORKSPACE (the file-tool jail), so a
# script edits the very files the agent wrote.

run_as    = "albert-scripts"   # scripts run setuid to this unprivileged user (when Albert can);
                               # missing on the host -> forkd warns and runs as the current user
run_group = "albert"           # child's PRIMARY group = the workspace-sharing group (setuid drops
                               # supplementary groups, so this is what opens the setgid 2770 workspace)

timeout_secs     = 30          # default per-run wall-clock limit
max_timeout_secs = 300         # ceiling a command may request
max_output_bytes = 65536       # stdout/stderr each capped to this
fsize_mb         = 64          # a script may not write a file larger than this
# mem_mb         = 512         # optional RLIMIT_AS (leave unset: too low breaks interpreters)
```

The isolation these knobs implement, and why `run_group` matters, is in
[architecture.md](architecture.md#isolation-three-layers).

### `connectors/search/search.toml`

Web search as an env-as-tools organ — `search.web { query, limit?, engine? }` →
`{ engine, query, count, results: [{ title, url, snippet }] }` (a clean hit list,
never raw HTML).

**The search system is chosen at two levels.** The manifest declares which engines
exist (one `[connector.engines.<name>]` table each, with its own settings) and which
is the `default_engine`; Albert overrides per call with `engine`. So one connector can
front several systems — DuckDuckGo today, Yandex next — and the model picks per query
with no config change.

```toml
[connector]
id   = "search"
type = "search"

default_engine = "ddg"   # answers when a call omits `engine`
timeout_secs   = 15      # per-search wall clock
default_limit  = 10      # hits per search when the caller doesn't say
max_limit      = 25      # ceiling; a call may ask for more than the default, up to this

[connector.engines.ddg]  # DuckDuckGo — no account or key
# region = "ru-ru"       # DDG `kl` locale; omitted = DDG's global default

# [connector.engines.yandex]   # planned; `enabled = false` shelves an engine
```

A single-engine deployment can still use the shorthand `backend = "ddg"` on
`[connector]`. A manifest with no engines, or a `default_engine` that isn't enabled,
fails at **startup** rather than at query time.

> **⚠ The `ddg` engine links the *system* libcurl.** Build hosts need
> `libcurl4-openssl-dev`; run hosts need `libcurl.so.4` — which is present wherever the
> `curl` command is (same package), so Albert's deployments already satisfy it (the
> forkd sandbox requires curl). The reason is measured, not incidental: DuckDuckGo's
> anti-bot answers reqwest/hyper with a `202` challenge page and zero results (with
> rustls *and* native-tls, over HTTP/1.1 *and* HTTP/2, with browser-like headers), and
> drops a **vendored** libcurl's handshake outright, while the system libcurl — the
> library the working `curl` command is a shell over — gets `200` and a full page. The
> block keys on the TLS fingerprint, which hyper cannot spoof, so every ready-made DDG
> crate wraps reqwest and hits the same wall.
>
> **Build trap:** `curl-sys` links the system library only if pkg-config finds it, else
> it silently vendors its own — and cargo caches that decision, so installing the dev
> package *after* a build needs `cargo clean -p curl-sys` before rebuilding. The
> connector logs the linked libcurl version at startup so a vendored build is visible
> immediately. An engine talking to a real API (Yandex) carries no such requirement.

### Subscription organs — `connectors/{transcribe,speak,imagegen}/*.toml`

Three organs ride on the ChatGPT-subscription token (`[subscription] auth_json` in
`albert.toml` — independent of `auth`, so the LLM can run on an API key while these use a
subscription). Each is **on while its manifest is present** and skipped at startup, with a
log line, when there is no token on disk. Remove a manifest to turn its organ off.

| manifest | tool | settings (all optional) |
|---|---|---|
| `transcribe/transcribe.toml` | `transcribe.run { path, language? }` → `{ text }` — any length; a long recording or a video is cut on its pauses (needs `ffmpeg`) and comes back with `[hh:mm:ss]` timecodes | `language` (default hint), `chunk_secs` (60–1380, default 300), `parallel_uploads` (1–8, default 4) |
| `speak/speak.toml` | `speak.run { text, voice? }` → `{ path }` — an Ogg/Opus voice note via a WebRTC call to ChatGPT Voice (needs outbound UDP) | `voice` (default `cove`; lower to higher: cove, spruce, arbor, ember, vale, breeze, juniper, sol, maple) |
| `imagegen/imagegen.toml` | `imagegen.run { prompt, size?, quality?, background?, images? }` → `{ path }` — gpt-image-2; `images` makes it an edit | `size`, `quality` (`low`…`high`, `auto`), `background` (`transparent`, `opaque`, `auto`) — a call always wins |

Voice messages in chat are heard through the same `transcribe` organ: the channel saves
the recording to the workspace and Albert hands it over (`transcribe.run { path }`), so
the organ's settings apply and a note of any length is heard. With no transcribe organ,
Albert answers a voice message by saying he can't hear right now.

The files land in the workspace; `chat.send_file` delivers them (`.ogg` as a voice note,
`.png` as a photo). The `imagegen` skill — the prompting guide — is offered only while the
imagegen organ is on.

### `connectors/mail/mail.toml` — off by default

A mailbox organ (IMAP read + SMTP send) is **wired in but disabled**: the factory is
registered in `main.rs`, but the repo ships only `mail.toml.example` — and Octo's
`from_config_file` loads only `*.toml`, so nothing is instantiated until you opt in.
To enable: copy the example to `mail.toml`, set the hosts, and put the credentials in
`.env`.

```toml
[connector]
id   = "mail"
type = "mail"
imap_host     = "imap.example.com"     # required (provider-neutral, no vendor default)
imap_port     = 993                    # implicit TLS; default 993
imap_user_env = "OCTO_MAIL_USER"       # env var holding the login
imap_pass_env = "OCTO_MAIL_PASS"       # env var holding the app-password
mailbox       = "INBOX"                # default INBOX
# SMTP defaults to the IMAP host + creds; override smtp_host/port/user/pass/from if they differ.
```

Commands (env-as-tools, via the dispatch tool — zero cogitator change):
`mail.cmd.list` / `read` / `send` / `reply`. Attachments come back as **metadata only**
(bytes never pass through the model). **Basic auth only** — REG.RU / Yandex / Fastmail
app-passwords work; Gmail does **not** (it needs XOAUTH2, not yet wired). Sending is a
real side effect — the decision to send is the cogitator's; confirm with the user first.

> Linking the mail crate pulls a second rustls provider (ring) alongside reqwest's
> aws-lc-rs, so `main()` installs one process-wide (`ensure_crypto_provider()`) — this
> runs even when mail is disabled, because the crate is compiled into the binary.

## What is runtime state (gitignored)

Machine-specific runtime data, not source — all gitignored: `state/` (scheduler
alarms, the SQLite history, the workspace, promoted artifacts) and
`config/connectors/telegram/telegram_acl.json` (the mutable ACL).

## A working config

For a standalone binary, start from `albert.dev.toml` with embedded memory. Keep
the prompts, skills and `config/` manifests beside it. Set `ALBERT_CONFIG` to
that file or copy it to `albert.toml` in the deployment directory. Connect your
first model in the web panel, or specify a provider and its credential reference
in the config. [Host deployment guide](deploy.md)

The production Compose stack seeds its own `albert.toml` with Kaeru MCP already
configured on the Compose network. [Docker Compose deployment](../contrib/deploy/README.md)

Set up Telegram under **Settings → Connections** if you use it. Edit the calendar
manifest and supply calendar credentials in `.env` to enable calendar integration.

## Model pool

For multiple model endpoints, live `/model` selection and bounded fallback, see
[the model pool guide](model-pool.md). Without `model_pool`, the existing single-model
configuration remains supported.

## Subagents

The `[subagents]` table controls delegation limits and enablement. See
[subagents.md](subagents.md) for grants, model subpools, lifecycle and examples.

## Conversation context

`[context]` allocates a service and response reserve within each effective model
window. The reserve is automatic by default and `reserve_tokens` overrides it
with a fixed value. The remaining space is split between fresh messages and
compacts. SQLite retains messages
and separate compacts. See [context.md](context.md) for defaults, `/compact`,
provider limits, estimation and failure behavior.

## Working scratchpad

The optional `[scratchpad]` table bounds the existing RAM task state, including
stages, child attempts and model-facing summaries. It introduces no SQL storage
or TTL. See [stage planning](stage-planning.md#bounds) for fields and defaults.
Child model routing is configured separately with opt-in `[[roles]]` in the
[model pool](model-pool.md#child-roles-and-reasoning-effort). Without these
profiles, children use only the current primary model.

## Authenticated tool context

`[authorization]` optionally configures `ingress_ttl_seconds` (default 3600),
`max_pending_invocations` (default 4096), and `decision_timeout_seconds` (default
30 for console human decisions). Telegram's manifest has its own optional
`approval_timeout_ms` (default 30000), `approval_max_cards` (128),
`approval_retention_seconds` (3600), and `approval_max_detail_bytes` (262144).
Card retention bounds the temporary button registry; it does not extend an
operation's approval deadline. These bound native host proofs and waiting;
they do not grant access or make model-authored provenance authoritative.
See [SQL connector](sql-connector.md) for its shipped manifest and data limits.
