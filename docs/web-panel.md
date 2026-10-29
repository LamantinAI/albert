# Web workspace

Albert includes a React + TypeScript client and an authenticated HTTP API in the
same binary. The Octo web connector owns the HTTP/SSE transport; Albert owns
accounts, permissions, configuration and session admission. SQLite and SQLx
remain shared with the existing internal database. Conversation catalogs and
journals remain in octo-history.

## First start

Start `albert` in a writable deployment directory, or set `ALBERT_CONFIG` to its
`albert.toml`. The file, a model, Telegram and a memory connection are not required
to open the panel. Open **http://127.0.0.1:9877/ui/**.

1. Sign in as **owner** with password **owner**.
2. Replace the initial password (at least 12 bytes); optionally rename the account.
3. Connect the first OpenAI-compatible model and supply its API key. Existing
   ChatGPT subscription credentials can also be used through model settings;
   `albert login` remains the interactive subscription sign-in command.
4. Start a permanent or temporary conversation.

The initial account is seeded once. Restarting never resets its password. Model
or memory failures leave the control plane available for repair. Ordinary users
are created by an owner and receive their own private profile and conversations.
They must replace their initial credentials on first login too.

## Accounts and conversations

JWT credentials use an HttpOnly, SameSite cookie, with no token in localStorage.
Mutating requests require the configured origin and `x-albert-csrf` from
`GET /api/v1/context`. Roles, disabled status and credential revocation are checked
against the database. SSE connections are also reauthenticated during delivery
and while idle. The signing key is persisted privately under `state/`.

Owners manage models, skills, configuration and users. A normal user can chat and
manage their own credentials, with a conversation-scoped scratchpad; the global
workspace, owner instructions, memory and connector tools are not granted to
ordinary browser users. Owner status does not bypass another person's private
session policy or a group's channel-bound policy.

Sessions are the existing octo-history resources, rather than a second web chat
database. Message pages default to the latest 10 messages, with opaque cursors
and stable snapshot boundaries. Lost message acceptance responses can be
recovered using the same client message ID. Retries do not interrupt twice or
create duplicate journal entries. Stop addresses one exact active attempt.

Temporary chats keep their transcript in RAM. Save promotes the same chat to a
permanent conversation with its complete journal; End discards an unsaved chat
and fences further work and output. Saving never merges it into a different chat.
Temporary conversations are lost on process restart.

## Existing Telegram installations

The bootstrap panel owner is attached automatically to the configured Telegram
owner's private canonical profile. Other existing private chats and groups remain
stored without creating web accounts or assigning their history to that owner.
This migration does not change Telegram ACLs, group modes or addressing rules.

For a UI-first installation, a subsequently configured and still-unbound Telegram
owner identity is attached before legacy import. An identity already assigned to
a different profile is never silently rebound or merged. Group sessions remain
accessible only through their originating connector and channel.

## Configuration and approval

Model settings include order, preference, capabilities, key references and the
configured role/subagent routing profiles. Keys are write only; new models get
independent key references unless the owner explicitly chooses a shared one.
Model and skill changes refresh the runtime; an unrelated refresh preserves a
model selected from Telegram. Running turns retain their model snapshots.

Skills can be imported as Markdown or ZIP, edited, enabled and disabled. Package
paths, size limits and command collisions are checked before replacing an
installed skill. The runtime catalog and cache are refreshed after a change.

Configuration and prompt documents have revisions. Validate, Save and Apply are
separate operations. A stale revision cannot overwrite newer settings. Add
connector creates a normal manifest in the configured connector directory and
initializes the topology if needed. Its `[connector] id` must match the selected
resource ID. Put credentials in the separate write-only credentials view and use
named environment references in the manifest.

Model, prompt and skill changes can be applied in process. General settings and
connector topology require **process** Apply. Application IDs and client request
IDs are persisted so retries recover their original outcome. Process Apply
shuts down gracefully and re-execs the binary without depending on systemd.
After restart, a requested connector that failed to load is reported as a failed
application rather than a successful model reload. Changing the panel's address
or origin requires opening its new address.

The Approvals page shows the owner's pending native SQL operations and explicit
Approve/Decline controls. The SQL executor still checks the original immutable
operation, principal, limits, expiry and session scope. The panel does not grant
the model permission to approve its own requests or bypass the SQL guardrails.

## Listener settings

```toml
[web]
enabled = true
bind = "127.0.0.1:9877"
# Set the browser-visible origin when using a reverse proxy or another interface.
# public_origin = "https://albert.example.org"
credential_ttl_seconds = 3600
login_attempts = 10
login_window_seconds = 60
password_workers = 2
max_body_bytes = 8388608
```

A non-loopback listener requires an explicit `public_origin`. HTTPS may terminate
at the deployment's reverse proxy. The API is rooted at `/api/v1`; its generated
OpenAPI document is `/api/v1/openapi.json`. Self-hosted Swagger UI is available at
`/ui/docs/index.html`, including cookie credentials and CSRF support. It loads no
CDN assets or external schema validator.

Configuration, `.env`, skills and `state/` must be writable by the service user;
`.env` and the signing key stay mode 0600. For containers, persist the writable
deployment directory as a directory mount. Individual file bind mounts and
read-only prompt mounts cannot support atomic replacement by the configuration
editor. Keep the existing state volume and credentials when changing mounts.

## Frontend development and embedding

The client lives in `frontend/`. Its design uses Lamantin's What thinks palette,
Spectral and IBM Plex Mono. Kaeru supplied visual and packaging patterns; its
current visualization client is plain JavaScript, so these React components are
new rather than a copied React application.

```sh
npm --prefix frontend ci
npm --prefix frontend run build:embedded
cargo build --locked
```

`ui-dist/` is the generated, versioned bundle. Cargo embeds it and verifies source
and asset fingerprints; it never invokes Node or downloads frontend packages.
An outdated bundle fails the Rust build with the rebuild command. Node is needed
only when changing the frontend. Fonts and Swagger assets include their licenses.

Frontend checks use `npm --prefix frontend test` and `test:browser`. The
`test:backend` suite requires a fresh isolated Albert instance and a local mock
provider via `ALBERT_E2E_URL` and `ALBERT_E2E_MODEL_ORIGIN`. It must not point at a
personal deployment or a paid model endpoint.
