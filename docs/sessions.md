# Sessions

A Session is one conversation with its own journal, compact checkpoints,
scratchpad and work scope. Its identity is independent of Telegram connector and
physical chat IDs. The executor is currently `host`: the existing Albert server,
workspace, files and tools. Other executor references require a later adapter.

The [host-only session messaging implementation](session-messaging.md) extends
this foundation with addressed inter-session messages, bounded context transfer
and explicit intermediate/final report destinations. Captured ingress replies
remain the default when no report override is selected.

That design records a default machine/connector execution location for each
session and the actual resolved target for each operation. The user's current
conversation transport and chosen intermediate/final report destinations remain
independent of execution placement. The implementation supports `host`; remote
placement awaits a device adapter. Report routing does not change existing
conversation selectors or native feedback lifetimes.

A private Session belongs to one canonical User. Verified external identities
can be linked by trusted configuration; sharing a role such as `owner` does not
link two people or grant access to someone else's Session. Each Telegram group
has exactly one Session bound to its exact connector and group channel. It cannot
be opened from a private chat, browser, another group or another connector,
including by an owner. Existing ACL and addressed/groupmode admission still apply.

The Session catalog and all permanent root/child journals are owned by
`octo-history` in the internal SQLite database. Albert's user bindings, private
channel selectors and content-free lifecycle markers share its pool with their
own migration registry. User SQL stores remain isolated. Linking a User never
renames native SQL principals or grants extra SQL permissions. Browser login,
HTTP control endpoints and a browser UI are separate work and are not enabled
by this stage.

## Names

`/new <title>` stores the supplied name. An unnamed saved private chat can receive
a short title after a completed conversation exchange. A separate, bounded,
tool-free model request uses a small excerpt of the initial user input; it does
not delay the main reply or load the full journal. If naming fails, a readable
excerpt provides the fallback. Nonempty titles are never automatically replaced.
Migration and startup do not backfill names or launch model jobs for old chats.

`/rename <title>` explicitly renames the current saved private chat without
interrupting work or changing its selected channel. A pending automatic title
cannot overwrite a manual rename. Names are single-line Unicode text, bounded
by the canonical catalog limit; temporary chats must be saved first.
Renaming is recorded as a host audit event, outside model context and previews.

## Telegram navigation

These commands are deterministic private-chat controls:

| Command | Result |
|---|---|
| `/sessions [page-cursor]` | Show named buttons for saved private chats, five per page, with pagination. |
| `/new [title]` | Create and open a separate persistent chat. |
| `/rename <title>` | Rename the current saved private chat without starting a model turn. |
| `/open <session-id>` | Continue an accessible private chat and preview its latest readable message. The bounded preview is not a history dump. |
| `/tmp` | Open a separate temporary conversation, retaining the return chat. Repeating it inside that excursion is a no-op. |
| `/save` | Promote the whole temporary conversation to a separate persistent chat with the same Session ID. It stays open. |
| `/end` | Return to the original chat. Discard an unsaved temporary conversation, or leave the saved one persistent. |

Select a chat by its button instead of copying its ID. Duplicate names are
disambiguated in the list while each button still addresses one exact Session.
`/open <session-id>` remains available for direct use. A new click on an older
valid list uses the channel's current selection; a repeated delivery of the same
click does not repeat navigation. Opening a chat does not start a model turn.
An expired list, including one from before a server restart, asks for `/sessions`
again. Changes to names appear when a list is next displayed. Private lists are
not available in groups and cannot open another User's conversations.

Selecting a different private chat does not move existing work or its canonical
results. Ordinary Telegram replies and implicit inter-session returns are shown
only while their addressed Session is active in that channel. An inactive
recipient still retains its journal message; suppressed output is not replayed
automatically on opening. Explicitly selected reports/notifications remain a
separate delivery intention and do not change the selected Session.
Leaving a view closes and removes its old typing/status presentation without
stopping the work or its tool journal. With progress streaming enabled, reopening
a still-running saved private Session on the same verified connector/channel/User
attaches a fresh typing and status presentation, including during compaction.
This requires no new
user message, model call or tool restart. New progress appears as it occurs;
old thoughts and tool lines are not replayed. Delayed events from the previous
view cannot close or recreate this new presentation. Saving or reopening the
already attached chat does not restart its status message.

Attachment changes only presentation: execution authority, diagnostics, history
and the captured final reply route stay with the original work. An idle or
finished Session does not start work when opened. A different channel is not
subscribed to the existing work's progress by this operation; cross-channel
progress subscriptions remain a separate extension.

Current Session identity and presentation generation are different: returning to
A permits an ordinary final for A, while an old detached status token stays
closed. Address resolution and canonical retry pinning never silently redirect
the result into whichever conversation happens to be selected later.

Opening previews a bounded tail of the canonical journal without a model call
or another journal write. Tool/control records and reasoning are skipped; media,
empty history, a service-only tail and a failed read have distinct notices.
Navigation previews use native Telegram formatted text with explicit blank lines
after technical headers. This avoids rich-block layout collapsing the visible
gap; ordinary generated answers continue to use rich rendering.
Inter-session responses are labelled as such: a later short response to another
conversation can be the latest record even when an earlier main answer contains
the full report. The preview does not replace or rewrite either answer.
`sessions.limits.open_preview_max_records` defaults to `32`, and
`open_preview_max_bytes` to `2048`. A preview/receipt from a departed selection
is suppressed before delivery rather than presented as the current chat.
Group chats cannot use these selectors. The model does not interpret navigation
commands or decide which Session receives a message.

Cancelling or interrupting work ends its typing indicator and progress trace on
the original transport. Manual compaction and resumed work use separate feedback
lifetimes; delayed events from a finished attempt cannot restart its indicator
or clear a newer attempt. Tool journals and independent subagents retain their
existing interruption policy.

Temporary transcripts, compacts, child journals and acceptance receipts remain
in retained RAM until Save. Quota exhaustion rejects new writes instead of
trimming history. Save atomically writes the catalog, complete journal graph,
stable public message/compact IDs and receipts with Albert's lifecycle metadata.
An unsaved restart leaves a content-free expired marker rather than recovering a
transcript. Ordinary input does not silently reuse an expired excursion.

Temporary Telegram media/files are currently rejected before download or save;
text inputs are not coalesced. Automatic artifact offload and child inspection file materialization are disabled
until Save, so tool results stay in the retained RAM journal. Large results may
require saving before further processing. A temporary chat does not automatically
capture Kaeru memory. Explicit external actions, deliberate memory writes and user SQL
effects have their own authority and are not rolled back by End. Neither this
mode nor Telegram cleanup promises provider-side erasure.

At End, Telegram cleanup targets only exact recorded native/request, reply and
status IDs for that route's excursion. Split/fallback/media and native approval
outputs are tracked too. Deletion is bounded, best effort and subject to Bot API
permissions and time limits; failures or unknown outcomes are reported honestly.
Telegram messages may remain visible. Active work in a saved conversation keeps
running after End; cleanup does not cancel or mute its later permanent output.

## Configuration

```toml
[history]
backend = "sqlite:state/history.db"
backup_dir = "state/history-backups"

[sessions.titles]
enabled = true
max_concurrent = 2
input_bytes = 1024
scan_records = 16
title_chars = 64
output_tokens = 1024
timeout_secs = 15
max_attempts = 1

[sessions.picker]
page_size = 5
ttl_secs = 600
max_lists = 128
max_receipts_per_list = 64

[sessions.limits]
temporary_max_sessions = 32
temporary_max_turns = 10000
temporary_max_bytes = 33554432
max_page_size = 100
telegram_tracking_max_scopes = 256
telegram_tracking_max_ids = 10000
telegram_cleanup_batches = 4
telegram_cleanup_timeout_ms = 3000
```

The Telegram picker defaults to five chats per page, with a Next page button
when more remain. `sessions.picker.page_size` is bounded by `max_page_size`;
other Session and message APIs retain their ten-item default. Tracking limits
bound the in-process Telegram cleanup registry; exceeding
them is a reported limitation, never permission to delete an unrecorded range.
The normal factory used by other Octo assemblies remains policy-free.

To link verified identities to a canonical User, use explicit host configuration:

```toml
[[sessions.identity_links]]
connector = "telegram"
actor = "123456789"
user_id = "personal-owner"
```

Only link identities whose ownership you have verified. A configured link cannot
silently replace an existing stored identity binding. Future browser adapters
must authenticate their own external identities before using this service.

## Existing history and backups

Before either internal migration chain upgrades an existing database, startup
creates a consistent SQLite snapshot in `backup_dir`, including committed WAL
content. A database whose schemas are already current is also backed up before
legacy channel data is moved. The completed preparatory snapshot is identified
by a content-free inventory of the remaining legacy keys, counts and row IDs;
unchanged leftovers reuse it across restarts. Unknown roots are therefore covered
before their later first-message migration without producing a new backup on
every restart. Snapshot creation uses a temporary file and atomic rename. Backup
failure stops startup before migration. Snapshots are retained for operator-managed
recovery and are not automatically pruned.

Migration is automatic; there is no per-channel mapping configuration or manual
SQL step. Every legacy `stdin` journal becomes the standard private Session of
`console` / `local`. Every nonzero numeric Telegram channel key becomes a standard
Session before input or reminders start: positive IDs are private conversations
owned by that connector's verified chat identity; negative IDs are group-bound
conversations. This is the historical Telegram channel convention used for this
cutover. It does not seed, alter or bypass ACLs, grant owner tools, or make an
inaccessible channel accessible.

Configured Telegram IDs are read without credentials or API calls, including
console mode while the Telegram token is absent. A console-only installation
without a connector manifest still migrates `stdin`.

With several configured Telegram connectors, old bare keys contain no bot or
connector identity. One deterministic custodian receives each whole old root:
the connector named `telegram`, or otherwise the lexicographically first ID.
The root is moved once. It is not copied or split across users/connectors. A
child's first host header retains its exact connector/channel binding; a child
from another configured connector moves to that connector's separate standard
Session, whose root can remain empty.

A remaining nonstandard legacy root is imported automatically on its first
verified native admission, before context loading or model work. Identity,
root/children, default binding and private selector commit in one transaction.
Repeated or concurrent admissions resolve the same Session. New channels with
no old data receive an empty standard Session. Reserved `session/` and
`subagent/` storage keys are never stolen by a physical channel using the same
string. A source already assigned to another standard Session is not reused.

Imports preserve all retained turns, IDs/timestamps, compact checkpoints,
acceptance receipts and references. Changes or write failures abort the whole
transaction. Existing group classification, anonymous-actor handling and
per-actor authority still apply to new messages. An old reminder can immediately
resolve the original default, independently of later private chat selection.

Old `subagent/<run-id>` journals stay separate children. Only their reserved key
and first host-authored header matching the exact origin/run ID can associate
them with a Session. A later JSON claim or JSON inside an ordinary user chat is
not provenance. Invalid headers remain untouched as technical orphans rather
than creating a user migration workflow. Previously deleted rows from capped
history cannot be recovered, and dangling/corrupt references are not repaired by
inventing transcript records.
