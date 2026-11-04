# Telegram account access

Open **Settings → Connections** and select the Telegram connector. Bot credentials
and personal login tokens are separate: the owner connects the bot, while each user links their
own Telegram identity to their panel account in the same connector settings.

## Connect and sign in

1. The owner pastes the BotFather token into the Telegram connector settings.
   Albert saves a write-only credential reference and restarts the connection.
   Readiness is confirmed only after Telegram identifies the running bot.
2. A signed-in panel user creates a personal login token. The command is shown
   once, with an expiry countdown and a copy action. The default lifetime is ten
   minutes. Creating a replacement revokes older unused tokens for that account
   and connector.
3. The user sends `/login <token>` in the bot's private chat. Login needs no model.
   Successful redemption binds the native Telegram actor and bot instance to the
   account; subsequent requests inherit its current role.

The token is random, single-use and connector/bot scoped. SQLite retains its digest,
account association and lifecycle timestamps, never the plaintext token. Login
attempts and issuance are rate-limited. Native preflight consumes login messages
before normal logs, digests, the event bus, history or model processing. Forwarded
login commands and group redemption are rejected. Albert attempts to delete the
credential-bearing Telegram message; Telegram delivery/deletion permissions still
apply. The browser keeps the displayed token only in component memory. Its copy action
supports local HTTP workspaces as well as secure browser origins.

## Permissions and revocation

Unknown private-chat users can only sign in. Linked users have ordinary private
conversation controls; model/configuration/restart administration requires the
owner role. The connector publishes native command menus for the current role.

In groups, explicit addressing is required. Unregistered guests receive text
answers without tools. Registered users retain their permitted conversational
capabilities, and the owner retains host tools. Personal session navigation is
private-chat only. Guests and unrelated registered users cannot preempt an
owner's active work or inherit its working checkpoint. Shared group context is a
public projection of the same journal: internal tool traces, action logs and
unmarked legacy compacts are excluded. New public compacts carry a host-created
provenance marker.

`/logout` or **Disconnect Telegram** revokes the binding, without deleting history
or logging out the browser. Disconnect also revokes unused login tokens for that
account and connector. Account disable/demotion invalidates affected active root
and child authority. Queued session work retains its original account and binding
provenance, and rechecks it before execution or delivery; legacy jobs never acquire
a new account grant merely because a historical identity is later linked. Pending SQL approvals remain tied to the original account
proof and session, so a new approver cannot revive a revoked operation. Those
proofs are retained in bounded memory for the configured pending-effect lifetime;
after a process restart, an old account-scoped approval must be requested again.

`sessions.telegram_login` configures token lifetime, retention and rate limits.
`sessions.limits.account_scope_ttl_seconds` and `max_account_scopes` bound retained
pending-effect proofs; the lifetime should cover the SQL connector's approval
window. Native tool dispatch checks current account state before execution.

## Existing installations

The upgrade requires one personal `/login`. Historical identity attribution is
kept separate from authorization: `owner_chat`, `admins` and allowlists no longer
grant access to Albert. Legacy private history moves automatically to the proven
account while retaining session IDs, the selected conversation, default routes,
original records, compacts, child journals and request receipts. Existing UI chats
remain intact; group sessions stay owned by their connector and channel.

No migration wizard, manual SQL mapping or duplicate empty conversation is
required. Identity data already owned by another account is never reassigned.
Later deployments retain the durable binding. Generic Octo consumers that do not
install Albert's account policy retain the legacy connector behavior.
