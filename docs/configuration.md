# Configuration

Albert is config-as-data: nothing tunable is hardcoded. There are three layers —
**secrets** (`.env`), **Albert-level config** (`albert.toml`), and **connector
manifests** (`config/`). Secrets only ever live in the environment; every config file
is safe to commit (it names secrets by their env-var name, never the value).

The rest of this page is a reference for each knob. For a **complete, working
configuration** you can copy and fill in, jump to [A working config](#a-working-config)
at the end — it is the exact set of files a running Albert uses.

## `.env` — secrets only

Loaded from the repo root (`../.env`, next to this crate). Holds secret *values* and,
optionally, the path to the config file:

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
# ALBERT_CONFIG=/path/to/albert.toml   # optional; default: next to the binary, else the crate dir
```

kaeru reads its own `KAERU_*` env (`KAERU_VAULT_PATH`, etc.) directly.

## `albert.toml` — Albert-level config

Located via `ALBERT_CONFIG`, else `albert.toml` next to the binary, else the crate dir.
Relative paths inside it resolve against the file's own directory.

```toml
auth  = "api_key"                        # "api_key" (default) | "subscription"
model = "deepseek/deepseek-v4-flash"
base_url = "https://openrouter.ai/api/v1"
openai_key_env = "ALBERT_OPENAI_KEY"     # the env var holding the LLM key

# multimodal    = true   # can the model see photos? omit -> heuristic:
                         # subscription = yes; api_key = guessed from the model name
# stream_status = true   # stream tool calls / thoughts into the chat while a
                         # turn runs (one in-place edited status message)

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
backend = "sqlite:state/history.db"   # "memory" | "file:<dir>" | "sqlite:<path>"

[scheduler]
state_path = "state/scheduler.json"   # where alarms persist (survives restart)

[agent]
max_tool_turns = 30      # max rig tool-loop rounds per message

[skills]
dir   = "skills"         # folder of <name>/SKILL.md (catalog shown each turn)
cache = 5                # LRU size for applied skill bodies
page  = 10               # skill_list page size + inline-catalog threshold (see below)

[code]
workspace = "state/workspace"   # ephemeral file-tool jail ($OCTO_CODE_WORKSPACE)
```

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

- **`albert login`** — a self-contained sign-in: it opens `auth.openai.com` in your
  browser (OAuth + PKCE), catches the redirect on `http://localhost:1455`, and writes
  the tokens to `auth_json`. On a headless / remote box where the browser can't reach
  that loopback, paste the redirect URL (or the bare code) at the prompt instead.
- **`codex login`** — since `auth_json` defaults to `~/.codex/auth.json`, an existing
  codex CLI login is reused as-is.

Either way, the access token becomes the `Authorization: Bearer`, the account id the
mandatory `ChatGPT-Account-ID` header. Albert **refreshes the access token in place**
before it expires (via the OAuth refresh token), so a session started days later just
works; only when the refresh token itself lapses do you re-run `albert login`. The
Codex `/responses` endpoint is streaming-only and rejects `system` messages / strict
schemas — Albert reconciles all of that transparently (see [`src/codex_http.rs`]).

[`src/codex_http.rs`]: ../src/codex_http.rs

> Note: driving a ChatGPT subscription from a third-party app uses OpenAI's Codex
> client credentials outside their officially-supported path — use it with your own
> subscription, at your own discretion.

### History backend

`[history] backend` chooses where the per-channel chat transcript lives:

- `"memory"` — in-RAM, lost on restart (dev default).
- `"file:<dir>"` — JSON per channel.
- `"sqlite:<path>"` — a migrated SQLite file (WAL), trimmed to a per-channel cap,
  **survives restarts** — this is what a real deployment uses, so Albert still
  remembers the dialogue after a reboot. The path is relative to the working dir.

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
  - **Scaling:** `[skills] page` (default 10) is both the `skill_list` page size and the
    inline threshold — a catalog up to `page` skills is shown in full each turn; a larger
    library collapses in the preamble to a count + a pointer, and the model finds skills
    with `skill_search <keywords>` (ranked by name/description) or `skill_list <page>`.
    So 100 skills cost a line of context, not a hundred.
- `[code] workspace` is the ephemeral scratch directory the file tools
  (`read/write/edit/list/glob/grep`) are **jailed** to, exported as
  `$OCTO_CODE_WORKSPACE`. The forkd runner (cwd) and the storage connector
  (promote/checkout) inherit the **same** directory by name, so they operate on the
  files the agent wrote with zero path coordination. octo-code creates it on first use.

### Cloud memory (`[clouds.*]`)

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

### Deterministic commands (`[commands]`)

Optional. Each `[commands."/name"]` binds a slash-command to a skill script run via
forkd and answers it as a **reflex** — before, and instead of, the LLM. No model call,
no agent turn: it's how someone drives Albert's skills fast, cheap, and predictably (the
deterministic counterpart to the built-in owner-only `/allow` `/deny` `/allowed`). `/help`
lists every configured command; it is always available once any command is declared.

```toml
[commands."/status"]
skill_path = "your-skill/scripts/run.py"   # relative to the skills dir (forkd skill_path)
args       = ["status"]                     # the script's subcommand + flags
reply      = "Status: {state} (uptime {uptime})."
help       = "service status"

[commands."/echo"]                            # /echo hello
skill_path   = "your-skill/scripts/run.py"
args         = ["echo", "{arg}"]              # a trailing {arg} takes the user's text
reply        = "You said: {text}"
help         = "echo the argument back"
owner_only   = true      # refuse outside the owner chat (default false)
timeout_secs = 15        # per-run wall-clock ceiling, clamped to forkd's max (default 15)
interpreter  = "python3" # default; set for non-python scripts
```

The named script must be a bundled skill script (`skill_path`, the same value forkd's
`skill_path` tool takes) and should print a **single JSON object** to stdout. `reply` is a
template: `{field}` / `{nested.field}` placeholders are filled from that JSON (an
unresolved path renders as `—`), and non-JSON stdout is echoed as-is. A single trailing
`{arg}` slot in `args` receives whatever the user typed after the command word; calling
such a command with no text returns a short usage hint instead of running. Command names
must start with `/`; a name that collides with a built-in reflex (`/start` `/help`
`/allow` `/deny` `/allowed`) is warned about at load and never fires. Long replies are
truncated to stay under Telegram's message limit.

## Connector manifests — `config/`

Connectors are config-driven through Octo's `from_config_file`: `main.rs` registers
each type's factory, then points the builder at `config/octo.toml`, which scans a
`connectors/` directory of per-connector manifests.

```
config/
├── octo.toml                          # [connectors] dir = "connectors"
└── connectors/
    ├── telegram/telegram.toml         # the chat channel (edge ACL)
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

```toml
[connector]
id        = "telegram"
type      = "telegram"
token_env = "OCTO_TELEGRAM_TOKEN"       # token value stays in .env
acl_path  = "telegram_acl.json"         # runtime allow-list (gitignored; next to this file)
owner_chat = 100000000                  # <- YOUR real chat id (ask @userinfobot); a placeholder locks you out
```

`owner_chat` is not optional in practice: the ACL starts **empty and drops every
chat, including you**, until an owner is seeded. A wrong/placeholder `owner_chat`
locks you out of your own bot — which is the safe failure (better than open). At
runtime the owner grows the list with the `/allow <id>` `/deny <id>` `/allowed`
commands, and the same owner-role gate exposes the `restart` tool.

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

The set of files above **is** a working configuration — the repo ships them with
placeholder values already in place (`config/connectors/*/*.toml`, `albert.toml`), so a
real deployment is those files with three edits:

1. `config/connectors/telegram/telegram.toml` → real `owner_chat` (from `@userinfobot`).
2. `config/connectors/calendar/calendar.toml` → your account (`login`, or the Google
   `collection` + client).
3. `.env` → the actual secret values (`ALBERT_OPENAI_KEY`, `OCTO_TELEGRAM_TOKEN`, the
   calendar password/OAuth secrets).

Everything else — the `albert.toml` above, the storage and forkd manifests — works
unchanged. See [deploy.md](deploy.md) for the host provisioning that pairs with it.
