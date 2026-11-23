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
3. Choose Sign in with OpenAI for a ChatGPT subscription, or connect an
   OpenAI-compatible model with an API key. The subscription wizard displays a
   one-time code and a link to OpenAI. Enter the code there and approve sign-in.
   Albert opens the account's model catalog as soon as authorization completes.
   `albert login` provides the same sign-in in the terminal.
4. If no memory backend is configured, choose built-in Kaeru memory or a dedicated
   Kaeru MCP server. A valid configuration, including the production Compose
   default, skips this step. HTTP supports an optional masked bearer token. Stdio
   uses named server credential references. Saving initializes the selected
   backend and creates the Albert initiative if needed. A connection failure
   keeps this choice and offers a retry.
5. Start a permanent or temporary conversation.

The initial account is seeded once. Restarting never resets its password. Model
or memory failures leave the control plane available for repair. Ordinary users
are created by an owner and receive their own private profile and conversations.
They must replace their initial credentials on first login too.

Existing agents keep their configured memory when upgraded. Owners can edit it
under **Settings → Agent → Memory**. A running agent keeps its current backend
until an explicit process restart; saving a different selection shows that
requirement. Memory settings use the dedicated `/memory` API resource and are
separate from the general-purpose MCP server registry.

## Agent settings

Agent fields and Memory share one **Save changes** action. It validates the complete
candidate before saving; a saved configuration is distinct from an applied one.
Use the explicit **Restart Albert** action when a process restart is required.
Restart requests retain their durable receipt when the connection is interrupted.

Fields display effective values, with **Default**, **Custom** and **Reset** keeping
inheritance visible. Duration and size selectors change only the display unit;
edits are converted to exact native integers before validation. Fields/Raw switches
keep pending edits, including the write-only token and unfinished environment
references. Raw memory edits and dedicated Memory edits that overlap are rejected
before mutation.

Combined saves check both the Agent document revision and the aggregate configuration
revision. They write one merged Agent document, preserving unrelated settings and
comments. Credentials, the document and setup metadata are separate stores: a
partial or uncertain result keeps the draft and offers explicit review, without
silently retrying a credential submission. Merely observing that a token exists
does not prove it equals a newly submitted value.

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
If an open chat is deleted or expires, the panel removes its sidebar entry and
returns to the new-chat screen without an error or replaying input. The stale URL
is replaced in browser history. Network and permission failures remain visible.
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

## Workspace navigation

The desktop sidebar collapses into a 64px navigation rail. Its grid track animates
so the chat recenters with the available workspace. The existing paw shrinks while
the fixed-proportion wordmark fades; the shell, conversation, editor and live
subscription remain mounted. Reduced-motion preferences disable these transitions.

The rail retains chat creation, temporary chat creation, all conversations,
settings, account and sign-out access, filtered by the existing capabilities.
Settings expands the sidebar and its existing disclosure without changing routes.
Hidden navigation is inert, and action popovers close before their triggers hide.
The mobile drawer keeps its full navigation, focus trap and Escape behavior.

Only a namespaced layout boolean is retained in browser storage. No account,
conversation, draft or credential data is stored with the preference. Storage
restrictions fall back to an expanded sidebar without preventing interaction.

## Conversation attachments

The composer accepts images, audio and video, including messages without text.
Selecting files uploads account-owned staging data without creating a conversation.
Image-only messages show a compact thumbnail without an empty text bubble; captions
appear above the preview. Markdown images also render automatically in the browser,
with no referrer header, and use the same original-image viewer. External image URLs
are never fetched or proxied by the server.
Ordered attachment IDs participate in message and first-run idempotency: a retry
recovers the accepted receipt before consulting staging. Accepted first-run input
can be reconstructed from its session-bound storage.

Permanent attachments are immutable BLOBs in the internal SQLite database, separate
from the agent workspace. Host-authored journal records carry ordered attachment
metadata; asset rows and durable journal references commit together. The additive
media migration leaves existing journals and previous migration checksums intact.
Temporary attachments remain in bounded RAM; Save promotes bytes and history in
one transaction, while End discards them under the session lifecycle fence.
Previously recorded textual media references are not reconstructed retroactively.

Files are delivered through authenticated same-origin session resources. Every
request checks current account/session authority, including HEAD and single-range
GET requests used for playback and seeking. Opaque IDs do not grant access to
another session. Filesystem paths and arbitrary remote URLs are never media API
resources. Downloads use safe content types, `nosniff` and sandbox headers;
unsupported active formats remain downloadable files rather than inline content.

Images open in a keyboard-accessible modal using the original bytes. Initial fit
never enlarges a small image; Original size allows natural-resolution scrolling.
Audio uses real decoded sample peaks with translucent ochre unplayed bars and
bright played bars. Decoding is serialized and gated by encoded size and duration;
large, unsupported or undecodable recordings retain normal playback and seeking.
Video uses native browser controls and metadata preloading. Codec availability
follows the browser.

Web images use the existing vision input; permanent audio follows the existing
transcription connector policy. Temporary audio remains playable without invoking
transcription or copying its content into the workspace. Video is retained as a
file, with no claim of native video understanding. Confirmed web file deliveries
and confirmed native `send_file` outputs are snapshotted into canonical history.
Native connector image/audio ingress is captured on admission. The pinned Telegram
transport supplies only a video thumbnail/caption, so original Telegram videos
require a future structured transport extension.

Uploads also accept nonempty documents, source files, archives and other binary
files. Recognized native media keep signature validation; unsupported formats use
`application/octet-stream` and download-only rendering, regardless of their
declared MIME type. Uploading a document does not execute it, unpack it, or place
its contents in model context.

For permanent owner conversations, the web host prepares a private workspace copy
under an opaque relative name before admission. A bounded attachment reference
and the preparation result stay with the canonical message, including its first
input. Albert uses ordinary file tools to inspect the copy when needed. Repeated
preparation can reuse an identical regular file but never replaces modified files
or follows symlinks. Failed preparation is explicit. First-input recovery reloads
the accepted reference rather than creating another file or changing its recorded
result. Workspace copies are mutable working files; immutable original downloads
continue to follow the conversation's attachment lifecycle.

Temporary and chat-only conversations can retain and download these attachments
without gaining host file-tool access or writing workspace copies. To inspect a
temporary document with host tools, save the conversation and resend it. This
keeps temporary bytes in bounded RAM until the existing Save/End lifecycle applies.

Limits are configured under `[sessions.media]`. Defaults allow eight web
attachments per message, at most one audio recording, and 32 MiB per file. Staging
expires after one hour, with a 64 MiB / 64-file account limit and a 256 MiB /
256-file shared RAM limit. Permanent storage is limited to 4 GiB and 100,000 files.
Byte and file-count limits both apply; changing them does not alter existing
attachment identities. Upload cancellation removes staging data, while accepted
assets follow their conversation's retention policy.

Older history loads automatically when its leading edge approaches the viewport.
The existing ten-message cursor pagination and reading position are retained, with
an inline loading status instead of a manual page button. Upload errors can be
dismissed and clear when attachments change or a message is accepted.

For recognized Telegram user messages, hovering over the message or focusing
its controls reveals **Telegram** beside the timestamp. The panel hides internal
source metadata. It removes generated media labels when the corresponding image
or audio is available to display, while preserving the user's caption.

## Composer skills, commands and voice

The left add button opens Attach file, Skills and Commands. Selected files and
skills appear above the input and can be removed before sending. The searchable
skill picker lists enabled, runtime-available skills; standing instructions are
already in force and are not selectable. Explicit selection currently requires
the owner, matching installed-skill management permissions. Skill bodies stay on
the server and load into the selected request; selection never grants extra tools.
Skill previews in the picker and settings show one line. Hover over a description
to read it in full.

Authenticated `GET /composer` supplies available choices, recording availability
and effective limits. Ordered skill IDs and revisions participate in message and
first-run fingerprints. Admission rechecks current permission, availability and
revision before inference. Exact accepted retries recover their receipts before
consulting a changed skill catalog. Journal records keep selected names and
revisions alongside the message, without copying instruction bodies or adding a
separate persistence table.

Commands use typed actions instead of injecting slash text into a web message.
Rename updates conversation metadata; New temporary chat opens an unsent draft.
Owner controls include Restart Albert and Reload models, including on the unsent
new-chat screen. Model reload validates and atomically replaces only the configured
pool, retaining active-turn snapshots and leaving skills and other agent settings
untouched. Invalid pools leave the current pool active. Configure Albert discovers
the enabled installed skill declaring `command: settings`, adds its exact ID and
revision to the current draft, and focuses the input without sending or changing
configuration automatically.

Global controls use persistent configuration-application receipts, separate from
session input receipts. Restart acceptance commits before shutdown is scheduled.
Repeated requests with the same identity recover the receipt; changed actions
under the same identity conflict. The client retains only request metadata across
uncertain delivery and reconnects. A crash after an in-memory change but before
result persistence is reported as interrupted with effects unknown, and the
accepted action is never automatically executed again. Process restart uses the
same confirmation as settings, including the temporary-chat lifetime notice.
`POST /sessions/{id}/compactions` accepts a client request ID and optional summary
instructions. It uses the native interrupt, compact and resume lifecycle, with a
separate idempotency namespace. Registered users can compact their own private
sessions. Retrying an accepted command does not interrupt the task again.

When an active connector accepts `transcribe.run`, an empty input offers a
microphone. Text or an attachment replaces it with Send. Browser capture requires
HTTPS or localhost, permission and a supported recorder format; an HTTP LAN origin
shows why recording is unavailable. Connector discovery does not promise external
provider health. Capture uses a real level meter, elapsed time, cancellation and
Stop. Stop prepares a playable preview; uploading and delivery require explicit
Send. Navigation, cancellation and late permission grants release microphone
tracks and audio resources. Unknown delivery outcomes retain original attachment
IDs and skill selections for an exact retry.

Recording defaults live under `[sessions.composer]`: four selected skills, 256 KiB
of combined selected instructions and a five-minute recording limit. Attachment
size/count limits still come from `[sessions.media]`. Temporary voice recordings
remain playable in RAM, with transcription unavailable until the chat is saved;
the composer states this before sending.

## Credential guard

Commands → Credentials opens an owner-only server-credential panel above the
composer. It nearly matches the input width and slides upward from behind its
fully outlined foreground in 150 ms, with reduced motion disabling the effect.
The header stays inside; compact check/close actions align with the masked value
field. It has an editable environment-variable name and Save/Update/Cancel actions. The same panel opens for the name-only `request_credential`
tool in an authorized private owner turn. Groups, ordinary accounts, delegated
children and routine runs gain no credential capability. Agent requests require
permanent host-tool scope; manual global owner entry also works from an unsent
or temporary draft, without creating or promoting a conversation.

Sensitive input uses a dedicated authenticated, CSRF-protected submission resource.
Its value goes directly to the host's existing `.env` writer. It is not a model
tool argument, normal chat message, attachment, memory record or progress/SSE
payload. Responses and events contain only names, request IDs and states. The
password stays in the form input rather than message state; browser storage keeps
only a request ID and submission phase, never a name or value. Multiline paste is
rejected before a password input can flatten it. Closing or switching scopes
clears the field. Stored values are never populated into the form.

A successful save reports the actual submitted name. Requests have bounded RAM
retention and terminal tombstones under `[sessions.credentials]`: defaults are
15 minutes for entry, five minutes for terminal replay, one hour for a pending
notification, 64 pending requests and 256 total entries. Values are nonempty
single-line text, at most 16 KiB by default. All in-process secret writers share
serialization so updates to different names do not lose one another. Competing
updates to the same name follow accepted write order; no compare-and-swap is
implied. A terminal replay returns its existing receipt without another write.

Agent-origin receipts wait for an idle session boundary and start at most one
name-only continuation; they do not interrupt busy work. Manual entry records a
name-only notice for the next normal turn and does not wake a model. Unsent drafts
have no journal and create no chat. Temporary notices stay in RAM. Credential
metadata events and GET snapshots remain usable after the originating turn ends.
Session closure, cancellation, expiry and current account authority fence pending
work. Completed saves are not rolled back by a late cancel.

Secret-file and journal commits are separate. A metadata failure leaves the
credential saved with notification pending; retrying notification never rewrites
the value. After an ambiguous write or process restart, the outcome is explicitly
unknown. The client checks metadata, does not automatically resend a value and
keeps uncertain input locked until status resolves or the request is cancelled.
Saving is separate from applying a connector's configuration.

The guard prevents value exposure through this handoff flow. It is not an OS or
broker boundary against every privileged tool or user process. Existing filesystem
permissions and execution policy still apply. Password masking does not encrypt
HTTP; retain the documented proxy/HTTPS and local-development hosting policy.
Values explicitly pasted into ordinary chat remain ordinary input; this feature
does not claim to erase them. Instructions prefer the panel and retain the raw
legacy setter only for values the owner already explicitly supplied for storage.

## Live activity

User message headings contain only their timestamp. Assistant work uses one heading
with a paw, the Albert name, a step count and an ochre arrow. Activity appears above
the streamed answer; the live preview and its committed record share this presentation
without duplicating the assistant identity. Work starts expanded and collapses automatically
when the attempt finishes; the arrow reveals its steps again. Ochre icons
distinguish memory, scratchpad calls, skills, connector operations, stages and subagents in a bounded,
scrollable list. Its thread connects the nodes without extending
past the first or last node. A returned tool result is distinct from a verified
stage; no completion percentage is inferred. Long model requests and waits for subagents
remain visible without adding status messages to the conversation journal.

After completion, the current view may retain this decoration above the latest
answer. Copy stays below the answer. The goal appears on its own line below
the operation. It is local to that open chat view and disappears on reload or reopening;
completed server snapshots are not restored as historical cards. An active attempt
still reconnects to live progress. No presentation history is written to the database.

Non-generic active phases remain visible beneath the activity header even before
any tool calls. Compaction shows the runtime's current part and elapsed time,
including after reopening a running conversation. The timer is separate from the
live status announcement so screen readers do not announce it every second.

`SessionState.progress` provides a bounded snapshot; scoped SSE progress deltas
update it without reloading message pages for every tool call. Reconnection and
terminal events reconcile against the authoritative session state. Raw tool
arguments, results and provider reasoning are excluded from this web projection.
An optional semantic `activity` category selects the tool icon. Core resolves it
from installed tool provenance and advertised connector operations, including
renamed connector instances. Display labels do not determine icons. Missing or
unknown categories use a generic wrench; existing stage and subagent kinds retain
their own icons. Category metadata adds no database records or argument content.
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

When a reply finishes, a view following the live output moves once to the beginning
of that reply. The transition waits for its canonical journal record and does not
move readers who scrolled elsewhere. Opening an already completed conversation does
not replay the transition. Activity expansion only scrolls into view on an explicit
user action, so live-to-recorded handoff preserves the reader's position.

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

## Shared settings lists

`SettingsList`, `SettingsRow` and `SettingsRowDetails` in the browser component
library own the entity-list presentation across Settings, connectors, models,
skills, people, credential names, approvals and subscription model selection.
Page adapters supply content and existing handlers; they do not define a second
row layout.

The component has navigation, managed and radio-selection modes. Navigation uses
real links. Managed rows keep their action buttons separate from their identity.
Selection uses a native radio and label, with optional Details outside the label
so inspecting a model does not change selection. Description and unavailable
reason are associated with the option for assistive technology. Approval-critical
information remains visible before decision actions.

One stylesheet defines typography, icon column, dividers, focus and the ochre
selected/preferred rail. Container queries adapt the component to its own width:
managed actions move beneath content in narrow containers, and detail grids stack
in smaller ones. This also applies inside a subscription dialog on a wide screen.
Dialog header/footer stay outside the scrollable catalog. Ordinary forms and
conversation history retain their own semantics.

## Connector settings

Connections is a list of configured connectors. Open a specific connector to edit its settings; the
shared page contains no bot credentials, login tokens or personal bindings.
Owners can create named instances and edit basic settings through typed fields,
with Raw available for advanced configuration. Unknown tables and comments are
preserved when fields are changed. Required settings, number bounds and choices
are checked before a field edit is accepted.

Search, browser, storage, script execution, SQL, calendar, mail, speech and image
connectors expose their supported scalar settings. Advanced retains the full
manual document selector, connector creation and Raw editor. Custom HTTP services
are managed there and omitted from the standard list and creation choices. Voice, transcription and image
connectors use the existing shared subscription; no additional OAuth flow is
introduced by the settings form.

Ordinary accounts see only the Telegram instances where personal linking is
available. Bot credentials and configuration remain owner-only. The directory
contains safe instance metadata, without environment references, filesystem paths,
raw configuration or other users' bindings.

## Telegram account access

Bot setup and personal access both live in the selected Telegram connector under **Settings → Connections**. The owner saves the BotFather credential; each user
issues their own login token and sends `/login <token>` to the bot privately.
The panel shows connection readiness, the user's own binding and revoke/disconnect
actions. Ordinary users cannot view or replace bot credentials.

Existing Telegram users sign in once after the account-authentication upgrade.
Their selected conversation and canonical journal remain intact. Later restarts
keep the durable binding. Legacy `owner_chat` is used only to attribute existing
history, never to grant access. Group sessions remain channel-bound and cannot be
opened as private sessions. See [Telegram account access](telegram-accounts.md).

The model pool also uses compact rows, with priority, preference and editing
actions kept alongside each model. Native disclosure markers use the same chevron
icons as the rest of the panel. Clipboard actions support local HTTP workspaces
where the asynchronous Clipboard API is unavailable.

Models and Skills share an interaction pattern: one Add action opens a choice of
creation methods, an editor occupies its own view with a return to the list, and
rows use the same Edit and Remove controls. Model preference and priority remain
model-specific; skill enablement remains skill-specific. Skill slash commands are
not displayed in the management list.
People uses the same focused editor and return actions when creating an account.
Replacing a credential brings its form into view and focuses the write-only value
field. Account includes a sign-out action for the current browser.

Unsaved editors are protected across in-panel navigation, browser Back/Forward and
local document switches. Keep editing retains the exact input, including invalid
field values. Discard changes continues the requested transition. Closing or
reloading the page uses the browser's native unsaved-change warning. Draft text,
passwords and tokens remain in their mounted forms; the shared guard tracks only
form identifiers and revision counters. No draft is persisted in browser storage.

Prompt documents use Validate and Save. Saved instructions are picked up by new
turns; they do not require configuration Apply. Configuration documents retain
their separate validation, save and runtime/process application steps. Failed
initial loads provide an error and retry action instead of an empty collection.


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

The Approvals page and the conversation composer share a typed owner decision
queue. Conversational confirmations pause the model loop, record its completed
tool result and resume the original private session exactly once after an explicit
Confirm or Decline. Closing the drawer only dismisses it. Expiry, cancellation,
revocation and session close suppress continuation. These requests remain bounded
in RAM; accepted decision receipts use the existing message journal.

Native protected actions use domain adapters; SQL is the first production adapter.
The executor checks immutable operation identity, grants, scope and expiry. A lost
reply remains unknown until the exact operation's authoritative status resolves
it; an empty pending queue is not evidence of success. The composer drawer uses
the credential panel's overlay style without changing transcript height. Requests
attach only to their validated original private session, never the selected chat.
Unbound legacy operations remain accessible through the authorized owner queue.

## Listener settings

```toml
[web]
enabled = true
bind = "127.0.0.1:9877"
# Optional: restrict requests to a particular browser-visible origin.
# public_origin = "https://albert.example.org"
credential_ttl_seconds = 3600
refresh_ttl_seconds = 2592000
login_attempts = 10
login_window_seconds = 60
password_workers = 2
max_body_bytes = 8388608
```

Each browser login has its own database-backed credential and opaque refresh
secret. The access JWT defaults to one hour; refresh defaults to an absolute
30-day lifetime from login. Both cookies are HttpOnly and SameSite=Strict, with
Secure applied under the existing HTTPS policy. Only the refresh digest is stored
in SQLite; its value is never returned to JavaScript or placed in browser storage.
The bounded refresh secret stays stable during its lifetime, while signed access
tokens are renewed. Shared CSRF and monotonic access expiry keep concurrent tabs
valid without overwriting another device's session.

`POST /auth/refresh` requires the refresh cookie, matching Origin and the custom
`x-albert-refresh: 1` header. The panel restores a refresh-only browser at startup,
renews before access expiry and on foreground return, and retries an authenticated
API/upload request at most once with its original idempotency key and body. A
temporary network/server error preserves the current UI. Invalid login credentials
and provider errors do not trigger global logout. Event streams recheck current
database authority, so a renewed login can continue streaming while revocation
still closes it.

Logging out revokes only that login and clears both cookies. Password changes,
role changes and account disabling invalidate previous credentials across devices.
The additive refresh migration preserves existing accounts, conversations and
unexpired access tokens; an existing browser signs in once to receive its first
refresh cookie.

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

The canonical API is `/api/v1`, with the Utoipa-generated document at
`/openapi.json` and the compatible `/api/v1/openapi.json` alias. The workspace also
exposes relative API aliases beneath `ui/api/v1`, so browser requests stay inside
the proxy mount. Plain self-hosted Swagger UI is available at `/docs/` and the
compatible `ui/docs/index.html` address, including cookie credentials and CSRF
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
existing model editor and returns to chat after saving. The OpenAI choice opens
the guided subscription wizard. In Models, **Add model** offers **API** or
**Subscription**; account reconnection is a separate action inside the subscription
picker.

After model setup, a fresh workspace with no explicit `[memory]` backend asks
the owner to choose one. A valid backend supplied by the deployment is adopted
on startup. For example, production Compose already chooses Kaeru MCP. If the
configured backend fails to initialize, Settings reports the error and offers a
retry.

Settings is the single directory of administrative sections. Agent contains
behavior fields and Raw only. Connections, Persona, Credentials, Models and API
reference remain sibling destinations, with a shared Settings navigation. The sidebar expands its section links above
the Settings button, leaving conversation history available. Keyboard navigation,
Escape and reduced-motion behavior are supported. Existing document links redirect to their dedicated destination; model
configuration remains available through its Advanced link.

## ChatGPT subscription sign-in

Only an owner who has changed the initial password can authorize a workspace
subscription. Copy the one-time code, open OpenAI's verification page and enter
the code there. Approve only a sign-in you started in Albert. Enable device-code
sign-in in ChatGPT security settings or ask your ChatGPT workspace admin to
allow it. See [Codex authentication](https://learn.chatgpt.com/docs/auth#login-on-headless-devices).

The CLI and panel share the Codex-compatible device authorization protocol. The
server polls OpenAI at the provider's interval and stops at the attempt deadline
or on cancellation. Only the owner browser session that started the attempt can
retrieve the one-time code or cancel it. Attempts are held in memory for up to 15
minutes. Restarting Albert clears pending attempts. The server keeps the provider polling
ID and token-exchange data private.

Authorization ends when credentials are connected. It does not add a model or
change the model pool. Albert then loads the authenticated account's subscription
catalog and shows the provider's model names and descriptions. Choose a model and
select **Use selected model** to add or reuse it and make it preferred for new
conversations. Existing model entries and conversations remain intact.

From Models, **Add model → Subscription** opens the same catalog directly when an
account is already connected. **Add model** adds or reuses the selected model
without changing an existing preference, then returns to the pool with **Done**.
An empty pool uses its first model as the default. Subscription entries share the
workspace credentials; adding a model does not start another authorization flow.
New entries have model-derived IDs; existing IDs and role references are preserved.
Closing the completed picker also refreshes the pool. While settings are being
applied, the picker keeps its progress visible; a failed application can be retried
without another sign-in.

Catalog failures keep the credentials connected. **Retry loading models** repeats
only the catalog request; it does not repeat authorization. Closing the picker or
restarting Albert also preserves credentials, and reopening the dialog resumes
model selection. A catalog snapshot is bound to its connection generation; account
replacement or process restart requires a fresh list before selection.

If selected model settings were saved but runtime application failed, **Retry
applying model settings** applies them without another login. Codex's `tool_mode`
metadata describes its own harness runtime and does not disable advertised models
in Albert. Capabilities are mapped from the catalog protocol rather than guessed
from model names. Closing or cancelling an unsaved attempt
prevents a late provider reply from installing credentials. Once saved, cancellation
cannot undo the installed account. Reconnecting requires confirmation because the
subscription is shared by this workspace.

Installation is serialized with refresh through the shared subscription handle.
Credentials are written atomically with restrictive permissions, stay off browser
storage and API responses, and are not included in provider errors. Existing model
entries, order and unrelated configurations remain intact. The preferred model
changes only through the explicit selection action. No provider model ID is
requested before authorization. The verification page works when Albert runs behind
a proxy or on a headless server.

See [local testing](local-testing.md) for isolated Docker end-to-end checks with
synthetic OAuth and model services.

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

The client lives in `web/albert-panel/`. Its neutral dark surfaces use golden ochre
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
npm --prefix web/albert-panel ci
npm --prefix web/albert-panel run build:embedded
cargo build --locked
```

Browser assets are generated under `web/ui-dist/` and are not committed. The web connector's
build script verifies source and asset fingerprints, then copies the bundle to
Cargo's `OUT_DIR` for embedding. Missing or outdated assets fail the production
build with the rebuild command. Cargo never invokes Node or installs packages.
Fonts and Swagger assets include their licenses.

The running API and TypeScript generator use the same Rust Utoipa schema. No
OpenAPI JSON snapshot is maintained. After changing HTTP DTOs or routes, run
`npm --prefix web/albert-panel run generate`; `check:api` verifies the committed
declarations against a fresh schema export. The schema-only tool and
`cargo test --locked -p albert-core` do not require browser assets.

Export the same schema directly without starting Albert:

```sh
cargo run --locked -p albert-connector-web --no-default-features --features openapi-export --bin export-openapi
```

For local frontend work, run `npm --prefix web/albert-panel run dev -- --host 0.0.0.0`.
Vite serves the client with HMR and proxies `/api` to the local Rust API. Set
`ALBERT_API_ORIGIN` for a different development API address; its default is
`http://127.0.0.1:9877`. The proxy preserves Host for the normal origin checks.
The binary continues to use its own embedded bundle until rebuilt.

Frontend checks use `npm --prefix web/albert-panel test` and `test:browser`. The
`test:backend` suite requires a fresh isolated Albert instance and a local mock
provider via `ALBERT_E2E_URL` and `ALBERT_E2E_MODEL_ORIGIN`. It must not point at a
personal deployment or a paid model endpoint.

## Operator and model roles

Models use the shared settings list. The Operator handles ordinary conversation;
selecting it saves the configured default and explicitly updates the live choice
for future turns, including when the same saved default is selected again after
a temporary runtime override. Editing another model or role preserves that override.
Running turns keep their immutable model snapshot.

Roles are editable assignments with a model, role name and description. They
configure the existing opt-in delegated-task routing: without an explicit model
or role assignment, a child uses its parent's base model. Assignments grant no
connectors, native tools or administrative authority.

Role edits use the current pool revision. A simple edit changes the primary
assignment and description while retaining existing fallback models and reasoning
effort. Advanced settings remain available for those compatibility fields. Model
removal requires explicitly reassigning or removing every role that references it;
failed validation leaves the persisted pool and credentials unchanged.



## Operator view

The sidebar's List/Operator control switches the conversation directory between
its existing list and a live card grid. Entering Operator collapses the desktop
sidebar automatically; it can still be expanded manually. Leaving Operator restores
the ordinary sidebar preference. New chat uses the same action in both
views and opens a draft; it does not create an empty catalog entry. Temporary
conversations remain available through the composer commands.

Cards preserve the existing session action menu and display the host, the last
three safe activity steps, and independent lamps for work, approval and credential
requests. Active work includes remaining subagents. Idle is distinct from an
explicit completed, failed, interrupted or cancelled outcome. Completion and
activity observations are process-local: restarting the runtime does not infer
success from recorded messages. The recent-conversation list also shows the work
lamp without changing its layout.

A single authenticated aggregate stream observes the caller's private sessions.
Its bounded membership follows visible cards and rows across catalog pages, rather
than stopping at the first loaded page. Navigation visibility changes refresh that
scope without requiring a scroll. Group histories are not included. Session metadata changes refresh the directory;
revocation and session closure fence subsequent updates. The bounded RAM-only
activity projection excludes tool arguments, results, arbitrary goal text and
provider reasoning, and remains observable when another chat is selected.

Model pool opens a compact resource disclosure. It is closed initially in ordinary
views and opens automatically in Operator. Each configured model appears once;
the current operator and role assignments sort first. The owner sees reported
model usage for the current UTC day. Missing metrics stay unknown; an explicit
reported zero remains zero. Cached input is a subset of input tokens and is never
added to the total again. Accounting covers normal turns, subagents, addressed
session work, compact and metadata calls at the provider boundary. Existing
conversation history is not used to reconstruct earlier usage.

The internal database migration adds separate usage metadata and a tracking-start
marker, without rewriting messages or compact records. Writes are bounded and do
not fail inference; incomplete reporting is indicated in the resource disclosure.

Subscription limits are a separate, initially expanded section shared by the
account rather than duplicated per model. They reuse Albert's subscription auth
handle and a bounded, identity-scoped cache. Requests are made only when an owner
opens the resource disclosure. Unsupported endpoints, missing windows and stale
observations are shown explicitly and never change model readiness or force an
OAuth reconnect. Standard Codex backend paths map to the sibling account-usage
endpoint, preserving a reverse-proxy prefix; arbitrary provider endpoints are not
guessed. Redirects are not followed.

The typed API resources are `GET /operator/snapshot`, `GET /operator/events`,
`GET /model-usage` and `GET /openai-subscription/limits`. Their contracts are exported
from Utoipa alongside the existing session and model APIs.


## MCP connections

Connections includes a separate MCP server registry. MCP is implemented by the
reusable `octo-mcp` client library and Albert's native tool adapters, independently
of Octo envelope connectors. Owners configure Streamable HTTP or stdio, credential
references, enablement and an explicit allowed tool subset. Discovery exposes
paginated metadata; schemas are fetched only when requested. New tools are never
enabled automatically. The dedicated Kaeru memory connection remains separate.

Server configuration lives in `config/mcp.toml` by default; `mcp_registry` can select
another configuration document. The typed editor preserves other servers and
client limits, uses exact revisions, and exposes raw TOML through Advanced. A new
connection discovers its tools before access is selected. Credentials are resolved
on the host and are excluded from tool definitions and transport error messages.

Native definitions are captured once per turn and retained across provider retry
and fallback. Each call rechecks the current account, enabled tool, server
generation and contract revision. Children receive only the individual MCP tool
names explicitly delegated from their parent's captured surface. Server annotations
do not grant authority. Cancelling a root call does not close a surviving child's
MCP connection; retired connections drain their active calls. Explicit server
shutdown revokes the entire server lifecycle. Indeterminate calls are never replayed.


## Deleting permanent conversations

The conversation menu offers Delete for owned permanent private chats, including
saved temporary chats. The confirmation explains that chat history, compacts,
attachments and managed attachment copies are deleted; other workspace files,
backups and completed external actions remain. Deleting the active conversation
returns to New chat. Group-bound and other users’ sessions cannot be deleted.

`DELETE /api/v1/sessions/{id}` requires the exact revision in `If-Match` and the
normal browser mutation protection. A changed revision returns 412 and requires
fresh confirmation. Repeated deletion of an owned tombstone resumes cleanup and
returns 204 only after completion. A 503 `cleanup_pending` response keeps an
explicit retry; a subsequent 404/410 read alone does not prove cleanup completed.

Erasure closes session authority and removes root/child records, compacts and
owned media in one transaction. Default channel chats are replaced by empty
Standard sessions and private selectors follow the replacement. Historical native
fences and minimal request identities remain, so delayed input and old first-run
requests cannot recreate the deleted conversation. Inter-session envelopes owned
by the conversation are erased without editing legitimate copies already sent to
another conversation.

A durable cleanup queue retains exact managed attachment UUIDs and their original
workspace roots. Existing-session file preparation records ownership before its
blocking writer starts and holds the session backend lease until that writer
finishes. Initial-message ownership commits with its accepted session. Cleanup
removes only reserved `media-<UUID>` names and scoped tool-result files, retries
after interruption, and runs even when no model is configured. Renamed or
independently created workspace files are outside the deletion scope.
