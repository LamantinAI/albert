# Configuration

Albert is config-as-data: nothing tunable is hardcoded. There are three layers —
**secrets** (`.env`), **Albert-level config** (`albert.toml`), and **connector
manifests** (`config/`). Secrets only ever live in the environment; every config file
is safe to commit (it names secrets by their env-var name, never the value).

## `.env` — secrets only

Loaded from the repo root (`../.env`, next to this crate). Holds secret *values* and,
optionally, the path to the config file:

```sh
ALBERT_OPENAI_KEY=sk-or-...            # LLM API key (named by openai_key_env in albert.toml)
OCTO_TELEGRAM_TOKEN=123456:ABC...      # Telegram bot token (named by token_env; omit -> console)
OCTO_YANDEX_APP_PASSWORD=...           # Yandex APP password (named by password_env in the calendar manifest)
# ALBERT_CONFIG=/path/to/albert.toml   # optional; default: next to the binary, else the crate dir
```

kaeru reads its own `KAERU_*` env (vault path, etc.) directly.

## `albert.toml` — Albert-level config

Located via `ALBERT_CONFIG`, else `albert.toml` next to the binary, else the crate dir.
Relative paths inside it resolve against the file's own directory.

```toml
model    = "deepseek/deepseek-v4-flash"
base_url = "https://openrouter.ai/api/v1"
openai_key_env = "ALBERT_OPENAI_KEY"     # the env var holding the LLM key

[prompt]
soul   = "soul.md"       # persona/tone           (hot-reloaded)
system = "system.md"     # operating instructions (hot-reloaded)

[reflection]
period_secs = 21600      # base memory-reflection routine cadence (6h)

[history]
backend = "memory"       # or "file:<dir>"

[scheduler]
state_path = "state/scheduler.json"   # where alarms persist (survives restart)

[agent]
max_tool_turns = 8       # max rig tool-loop rounds per message
```

### Auth: API key vs. ChatGPT subscription

`auth` selects how the LLM call authenticates:

- `auth = "api_key"` (default) — an API key against `base_url` (OpenRouter or any
  OpenAI-compatible endpoint). The key lives in the env var named by
  `openai_key_env` (default `ALBERT_OPENAI_KEY`). This is the original behaviour.
- `auth = "subscription"` — a **ChatGPT subscription** (Plus/Pro/Team/…): model
  calls go to the OpenAI **Codex backend** using OAuth tokens instead of a metered
  API key. No `openai_key_env` is needed.

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

### Prompt files (`soul.md`, `system.md`)

`soul.md` is the persona; `system.md` the operating protocol (memory, reminders,
scratchpad). Both are loaded into RAM once and re-read only when their **mtime**
changes — edit and save, no restart, and no re-reading big files on every request.
A missing file falls back to a terse embedded default.

### Cloud memory (`[clouds.*]`)

Optional. By default Albert's memory (kaeru) is **local-only** and the
share/pull/cloud_recall tools are not shown to the model. Declare one or more
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
`kaeru_cloud_links`, `kaeru_sync_review`); with none it stays local-only. Design:
[`docs/kaeru-cloud-handoff.md`](kaeru-cloud-handoff.md).

## Connector manifests — `config/`

Connectors (the Telegram channel, the calendar) are config-driven through Octo's
`from_config_file`: `main.rs` registers each type's factory, then points the builder at
`config/octo.toml`, which scans a `connectors/` directory of per-connector manifests.

```
config/
├── octo.toml                          # [connectors] dir = "connectors"
└── connectors/
    ├── telegram/telegram.toml
    └── calendar/calendar.toml
```

This path runs when a Telegram token is present; without one Albert uses a console
channel (and does not load the calendar).

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
commands.

### `connectors/calendar/calendar.toml`

```toml
[connector]
id       = "calendar"
type     = "caldav"
base_url = "https://caldav.yandex.ru"   # the calendar collection is auto-discovered
# calendar = "My events"                # optional: narrow to one calendar by display name

auth         = "basic"                  # basic app-password (Yandex/Fastmail/Nextcloud/iCloud)
login        = "you@yandex.ru"
password_env = "OCTO_YANDEX_APP_PASSWORD"   # APP password in the env, NOT the account password
```

Two calendars later = a second manifest with a different `id` / `login` /
`password_env` (e.g. `calendar-work`, `calendar-personal`); same `type = "caldav"`.
Google is OAuth2-only and not wired end-to-end yet.

## What is runtime state (gitignored)

`state/` (scheduler alarms) and `config/connectors/telegram/telegram_acl.json` (the
mutable ACL) are machine-specific runtime data, not source — both are gitignored.
