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

User messages and assistant answers have a Copy action at the right beneath the text. It copies
the original message text, including Markdown, without author labels or timestamps.
Clipboard failure is shown beside the action; copying does not change the journal.

Technical journal records are omitted from the ordinary chat display. History
reads skip service-only pages within a bounded scan, preserving the snapshot and
an older-page cursor when the scan budget is reached. Durable records stay intact.

Temporary chats keep their transcript in RAM. Save promotes the same chat to a
permanent conversation with its complete journal; End discards an unsaved chat
and fences further work and output. Saving never merges it into a different chat.
Temporary conversations are lost on process restart.

Opening New conversation shows a local draft without creating a session or a
sidebar entry. The first Send accepts the input and its permanent or temporary
execution context atomically through `POST /api/v1/session-starts`. The internal
context stays out of the catalog and automatic naming until its first answer or
terminal error is committed. A provisional route reconnects to accepted work
without publishing an empty or Untitled conversation. The welcome view transitions
into that same conversation without remounting it when its public route appears.

Temporary conversations receive the same automatic first-turn naming as permanent
conversations. Their generated name stays in RAM and is promoted with the chat;
metadata notifications update the open workspace without a reload. Explicit saved
names and lifecycle changes take precedence over delayed generation.

The client-generated request ID identifies this first input. Retrying the same
request recovers its receipt; changing its text or retention under that ID is a
conflict. `GET /api/v1/session-starts/{id}` reconciles a lost acceptance response,
and `POST /api/v1/session-starts/{id}/cancellations` cancels that start. Private
ownership and tool grants apply before publication too. Pending starts are bounded;
restarting records interrupted permanent work honestly instead of replaying its
external effects. Unsaved temporary work still expires on restart. First-input
metadata contains no temporary transcript.

The draft stays in browser memory across validation errors and uncertain delivery.
Changing its retention choice preserves the text; explicitly choosing New chat
clears it. Conversation actions open from the ellipsis beside their directory row;
the conversation itself has no duplicate title/header or action toolbar. The row
link and action button are separate controls. Menus remain visible outside the
scrolling list and support keyboard use and the mobile drawer. Save asks for an
optional conversation name and promotes the transcript and name atomically. Rename
is available after saving; temporary deletion uses the panel's confirmation dialog.
Actions can target a background conversation without selecting it. Ending a
background chat leaves the current route intact, including when navigation occurs
before the request completes. Menus render immediately from directory metadata,
without a request on open. Selecting an action fetches current metadata before
its dialog opens, checking retention and state as well as the mutation revision;
a lost Save receipt retries the same promotion key, payload and revision.

Opening a chat reads that resource's state and message pages. The directory cache
retains its loaded pages, cursor, search and scroll position. Authoritative metadata
patches existing rows with revision checks; confirmed first-pass publication adds a
new row once. Ended rows cannot reappear from delayed page replies. Ordinary
navigation changes the active row without reloading the catalog or adding a loading
line. Explicit full catalog reload remains separate from conversation reads.

## Live activity

The conversation shows the current operation and its elapsed time beside a paw,
a step count and an ochre arrow. Work starts expanded and collapses automatically
when the attempt finishes; the arrow reveals its steps again. Ochre icons
distinguish tools, scratchpad stages, subagents and native skill tools in a bounded,
scrollable list. Its thread connects the nodes without extending
past the first or last node. A returned tool result is distinct from a verified
stage; no completion percentage is inferred. Long model requests and waits for subagents
remain visible without adding status messages to the conversation journal.

After completion, the current view may retain this decoration beneath the latest
answer, sharing one footer row with Copy. The goal appears on its own line below
the operation. It is local to that open chat view and disappears on reload or reopening;
completed server snapshots are not restored as historical cards. An active attempt
still reconnects to live progress. No presentation history is written to the database.

`SessionState.progress` provides a bounded snapshot; scoped SSE progress deltas
update it without reloading message pages for every tool call. Reconnection and
terminal events reconcile against the authoritative session state. Raw tool
arguments, results and provider reasoning are excluded from this web projection.
Selected display fields, such as a bounded scratchpad goal in quotation marks,
may be shown as operation labels. Stage labels
are visible only to the private session's owner. Live progress follows the existing
status-stream setting; it does not change Telegram's string status protocol.

Selection menus and confirmation dialogs use the panel's own appearance. Ending a
temporary conversation requires explicit confirmation; cancelling the dialog sends
no mutation. Keyboard navigation, Escape and focus return are supported. Conversation menus
defer blur-triggered closure until native browser focus transfer has finished.

## Streamed answers

Private web conversations receive actual model text deltas through the session's
SSE subscription. A matching `SessionState.text` snapshot restores an active preview
after reconnecting. Each preview has an attempt, generation and UTF-8 byte offset;
duplicates are ignored and gaps trigger snapshot reconciliation. A new provider
round or retry gets a fresh generation. Only user-facing root text is projected;
provider reasoning, tool arguments, compaction and subagent output are excluded.

The observing adapter drains the provider stream below the existing budget and
recovery wrapper. The ordinary tool loop, completion hooks, usage accounting and
final journal commit remain authoritative. Tool IDs and provider-specific opaque
reasoning are retained internally for subsequent rounds. Incomplete transport EOF
is an error rather than a successful answer. API-key and subscription transports
use their own terminal markers.

A solid terminal-style ochre block follows incoming text. Received live bursts
flow through a short bounded display buffer; newly displayed token spans glow
ochre, then settle into the normal text color within roughly 260 ms. Decorative
character substitution is not used. Completed answers are never replayed as
simulated typing. Reduced-motion mode uses a static cursor. The
preview renders escaped Markdown and is replaced by exactly the committed message
IDs, rather than guessing from timestamps or neighboring records. Copy becomes
available on the committed answer. Chunks are coalesced and bounded; snapshots cap
preview text at 256 KiB while the final journal retains the complete answer. Text
deltas and completed visual activity are never written as per-token SQL records.

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
# Optional: restrict requests to a particular browser-visible origin.
# public_origin = "https://albert.example.org"
credential_ttl_seconds = 3600
login_attempts = 10
login_window_seconds = 60
password_workers = 2
max_body_bytes = 8388608
```

Direct access and HTTPS termination work automatically when the proxy preserves
the browser's Host authority. Origin must match that authority; CSRF checks remain
active, and forwarded headers are not trusted. HTTPS browser requests issue Secure
cookies. No public URL setting is required for this topology. The optional
`public_origin` pins a particular origin and also supports a proxy that deliberately
rewrites Host.

Complete initial setup with the known `owner` password through localhost, a direct
IP address or matching HTTPS origin. After replacing that password, matching HTTP
hostnames work automatically too. This prevents a rebound HTTP DNS hostname from
claiming the initial account with known credentials.

The panel can be mounted beneath a path such as `/tools/assistant`, without an
Albert path setting. A proxy may strip that mount or preserve it before forwarding.
Opening the mount, with or without a trailing slash, enters its `ui/` workspace.
Assets, navigation, API calls, live events and API reference links resolve within
the browser-visible workspace; refreshing a nested page keeps the same mount.

The canonical API is `/api/v1`, with a generated document at
`/api/v1/openapi.json`. The workspace also exposes relative API aliases beneath
`ui/api/v1`, so browser requests stay inside the proxy mount. Self-hosted Swagger
UI is available at `ui/docs/index.html`, including cookie credentials and CSRF
support. Its server URL resolves relative to the hosted document. It loads no CDN
assets or external schema validator.

Configuration, `.env`, skills and `state/` must be writable by the service user;
`.env` and the signing key stay mode 0600. For containers, persist the writable
deployment directory as a directory mount. Individual file bind mounts and
read-only prompt mounts cannot support atomic replacement by the configuration
editor. Keep the existing state volume and credentials when changing mounts.

## First-time setup and settings navigation

The first authenticated visit with initial credentials explicitly opens profile
setup. After choosing sign-in credentials, the user continues to a chat. When no
model is connected, the chat shows prominent model-connection choices instead
of relying on a small configuration warning. The API-key choice opens the
existing model editor and returns to chat after saving. The OpenAI sign-in card
is a design placeholder marked Coming soon; it does not start authentication.

Settings is the single directory of administrative sections. Agent contains
behavior fields and Raw only. Connections, Persona, Credentials, Models and API
reference remain sibling destinations, with a shared Settings navigation. The sidebar expands its section links above
the Settings button, leaving conversation history available. Keyboard navigation,
Escape and reduced-motion behavior are supported. Existing document links redirect to their dedicated destination; model
configuration remains available through its Raw link.

## Agent settings and draft editing

The sidebar dedicates its available height to scrollable conversation history
without a fixed display cap, with older pages available in place. All
conversations provides title search over the loaded catalog. Workspace
administration is available through Settings, and the profile opens My account.
The compact Model pool indicator remains available without a redundant route-name
header. Temporary chats use the Save/Delete menu described above.

Agent settings offer grouped fields and a Raw TOML view over the same unsaved
draft. Optional fields retain their inherited defaults until explicitly changed.
The owner-only `POST /api/v1/configuration/{id}/drafts` endpoint projects or edits
allowlisted agent settings without writing files or applying runtime changes.
It checks the saved revision and preserves unrelated TOML syntax and comments.
Removing an override retains attached comments as standalone trailing comments.
Inline secrets and unsupported structures remain on the protected raw workflow.

Switching views flushes pending field edits; stale replies cannot replace a
newer draft. Validation still checks exact content before revision-checked Save,
and Apply remains a separate operation on the saved revision. Editing is locked
while a save is pending. Other configuration documents retain their specialized
or raw editors; model and credential controls remain write-only for secrets.

## Frontend development and embedding

The client lives in `frontend/`. Its neutral dark surfaces use golden ochre
accents, Space Grotesk and IBM Plex Mono, with dot-matrix identity artwork and
compact pixel branding. The sidebar logo, mobile header logo and favicon remain static; chat activity
has a separate animated paw. The large chat wordmark periodically dims as one
unit, then its letters relight from left to right. Page-local loading boundaries
keep the navigation and brand visible while route code loads; restrained content
transitions and reserved editor space reduce layout jumps. Animations respect
reduced-motion preferences. A shared session
directory keeps navigation and conversations synchronized without a second
history store. All controls use the same authenticated API as before.

```sh
npm --prefix frontend ci
npm --prefix frontend run build:embedded
cargo build --locked
```

`ui-dist/` is the generated, versioned bundle. Cargo embeds it and verifies source
and asset fingerprints; it never invokes Node or downloads frontend packages.
An outdated bundle fails the Rust build with the rebuild command. Node is needed
only when changing the frontend. Fonts and Swagger assets include their licenses.

For local frontend work, run `npm --prefix frontend run dev -- --host 0.0.0.0`.
Vite serves the client with HMR and proxies `/api` to the local Rust API. Set
`ALBERT_API_ORIGIN` for a different development API address; its default is
`http://127.0.0.1:9877`. The proxy preserves Host for the normal origin checks.
The binary continues to use the versioned embedded bundle.

Frontend checks use `npm --prefix frontend test` and `test:browser`. The
`test:backend` suite requires a fresh isolated Albert instance and a local mock
provider via `ALBERT_E2E_URL` and `ALBERT_E2E_MODEL_ORIGIN`. It must not point at a
personal deployment or a paid model endpoint.
