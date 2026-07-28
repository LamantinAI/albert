---
name: self-config
description: when the owner asks you to change your own setup — edit a setting, add/remove or reconfigure a connector, tweak your persona or operating instructions, add a skill, or adjust a config value. Triggers «поменяй свой конфиг», «добавь коннектор», «поправь промпт», «включи/выключи…», «измени настройку».
---

You can read and edit your own configuration with the owner-only tools `config_read`,
`config_list`, `config_write`, `config_edit`, and apply changes with `restart`. These
reach your REAL deploy files (not your scratch workspace). Follow this exactly — a
wrong edit can break a connector or lock you out.

## First rule: discover, never assume

Your memory of your own layout is unreliable — **check with the tools before editing.**
Start with `config_list` (no path = the top level, or a dir like
`config/connectors`). Then `config_read` the file you intend to change. Do not write a
path you haven't confirmed exists.

## The real layout

- `albert.toml` — main settings: `model`, `auth`, `timezone`, `[history]`,
  `[scheduler]`, `[agent] max_tool_turns`, `[skills]`, `[code] workspace`, `[clouds]`.
- `soul.md` — your persona. `system.md` — your operating instructions.
- `config/octo.toml` — points at the connectors directory (rarely touched).
- `config/connectors/<id>/<id>.toml` — **one manifest per connector, named by its id**
  (e.g. `telegram/telegram.toml`, `search/search.toml`, `jira/jira.toml`) — NOT
  `connector.toml`. Confirm the real name with `config_list config/connectors/<id>`.
- `skills/<name>/SKILL.md` — skills (like this one).

Secrets live in `.env` — handled ONLY by the dedicated `config_set_secret` /
`config_list_secrets` tools (the read/write/edit tools refuse it). Also off-limits to
those tools: the `albert` binary, `state/`, the kaeru vault.

## Secrets

A manifest names a secret by its env-var (e.g. `${secret.jira_token}` → `JIRA_TOKEN`),
never the value. To supply the value yourself:

- `config_list_secrets` → the NAMES currently set (never values) — check what's missing.
- `config_set_secret { name, value }` → add or replace one secret. Write-only: you can
  set a secret but never read existing ones back. Apply with `restart` afterwards.
- **Never repeat a secret value in your reply** — not even to confirm; say "set" and the
  name only. The owner gave it to you in confidence.

This lets you wire a new connector end to end: write its manifest, set its token, restart.

## Making a change

1. `config_list` / `config_read` to see the current, real content.
2. Edit:
   - a small change → `config_edit { path, old, new }` (`old` must be unique — include
     enough surrounding text).
   - a whole new file (e.g. a new connector manifest) → `config_write { path, content }`.
3. **Apply it:**
   - `soul.md` / `system.md` are **hot-reloaded** — editing them takes effect on your
     next turn, **no restart**.
   - `albert.toml` and any `config/connectors/**` manifest are read at startup — apply
     with `restart`: `restart { target: "<connector id>" }` reloads one connector's
     manifest; `restart { target: "process" }` for an `albert.toml` change.
4. Tell the owner what you changed **before** you restart, then confirm it took.

## Guardrails

- **One change at a time**, then verify — don't batch edits you can't check.
- A manifest's `type` must be a connector type the binary already knows (telegram,
  caldav, storage, forkd, search, http, mail). You can add a manifest for an existing
  type (e.g. another `type = "http"` API), but a brand-new connector type needs a code
  change and rebuild — which you cannot do; tell the owner instead.
- Secrets: set them with `config_set_secret`, and never echo a value back (see Secrets).
- If you're unsure whether a change is safe, describe it to the owner and ask before
  applying — don't guess.
