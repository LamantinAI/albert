---
name: self-config
command: settings
command_about: Change my setup
command_owner: true
description: when the owner asks you to change your own setup — edit a setting, configure model-pool members or subagent role/model/effort assignments, add/remove or reconfigure a connector, tweak your persona or operating instructions, or add a skill. Triggers "change your config", "configure subagents", "assign models", "add a connector", "fix the prompt", "turn on/off …", "change a setting".
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
  `[scheduler]`, `[agent] max_tool_turns`, `[skills]`, `[code] workspace`, `[clouds]`,
  `[scratchpad]` limits, and the top-level `model_pool` path.
- The file named by `albert.toml`'s `model_pool` (commonly `config/models.toml`) —
  runtime `[[models]]` members, `[[roles]]` delegation profiles, reasoning support
  and fallback limits. Discover the configured path; do not assume the example.
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

## Model pool and subagent assignments

Model routing is **runtime configuration**, not an instruction in `system.md`.
Writing "use model X for research" in a prompt does not register a role or make
that model available to children. Do not report routing as configured after only
editing the prompt or writing a plan assignment.

1. Read `albert.toml` and locate `model_pool`. Discover and read that TOML file
   with `config_list` / `config_read`. Without `model_pool`, Albert has a single
   legacy member named `default`; introducing a pool also requires setting that
   top-level path and applying the `albert.toml` change with a process restart.
2. Inspect `model_select` without `model_id` for root pool IDs, and
   `subagent { action: "capabilities" }` for currently loaded child defaults,
   allowed models and roles. These are different views: a root pool member is
   not automatically available for delegation.
3. Add or edit `[[roles]]` in the pool TOML using the owner's requested mapping.
   Each profile requires `id`, `description` and a nonempty ordered `models` list
   of existing `[[models]].id` values. For example, using an actual configured ID:

   ```toml
   [[roles]]
   id = "research"
   description = "Research using the owner's selected models"
   models = ["existing-model-id"]
   ```

   `reasoning_effort` is optional. Set it only when requested and supported by
   every selected model. Support is declared inside each relevant `[[models]]`
   entry as `reasoning = { wire = "reasoning_object", supported_efforts = [...] }`
   or the API's `reasoning_effort` wire format. Subscription Responses requires
   `reasoning_object`. Do not invent supported values or infer them from names.
   Leave effort unset when it is not needed; do not silently change providers,
   secrets, root defaults or unrelated model settings to configure a role.
4. Read the edited file back. A saved file is not yet a loaded configuration.
   For an existing pool, tell the owner to run **`/model reload`**; it validates
   and atomically loads the pool without a process restart. An invalid reload
   leaves the old pool active. A separately owner-authorized process restart
   also loads it, but discards the RAM scratchpad and running child handles.
   `model_select` only selects a root preference: it cannot edit or reload a
   pool. Do not pass `reload` as a model ID or pretend writing a chat command
   executes it.
5. After application, inspect `subagent` capabilities again and compare its
   loaded roles/model IDs/efforts with the requested mapping. Only then report
   routing as active. Until then say "saved; awaiting /model reload". Capability
   inspection does not need a paid trial child run.

With no configured roles, new children can use only the current selected primary
model. With roles, omitting both role and models still uses only that primary;
explicit role selection uses its ordered subpool. Roles grant **no connectors or
native tools**. A role's explicit model override must stay within that role.
Each running child retains its model/effort snapshot after a reload.

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
   - The existing model-pool TOML needs the owner's `/model reload`, then a fresh
     `subagent` capability check; follow the model-pool procedure above.
   - Skill metadata and applied bodies are cached. A process restart reliably
     loads an edited `SKILL.md` and refreshes its catalog entry.
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
