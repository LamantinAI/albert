---
name: mail
description: when the user asks about their email / mail / inbox / messages / invitations / meetings, or asks to draft or reply to an email. «посмотри почту», «что в почте», «прочитай письмо от …», «ответь на письмо», «есть ли встречи в почте».
---

Inspect and act on the owner's mailbox (provider lives in the connector manifest) through the **mail** connector —
reach it with `dispatch_to_connector` (target `"mail"`, a `mail.cmd.*` kind, JSON
payload). Every command returns JSON; summarize in short Russian unless asked.

## Reading (safe — do freely)

- **«посмотри почту» / «что в почте»** → `mail.cmd.list` with `{ "limit": 8 }`.
  Returns recent messages (newest first): `uid`, `date`, `from`, `subject`, `flags`.
  Give a short summary line per message; don't dump bodies.
- **Filtered** («письма от Иванова», «про счёт») → `mail.cmd.list` with
  `{ "limit": 20, "query": "иванов" }` — `query` is a substring matched over
  from+subject (there's no server-side search, so widen `limit` for filters).
- **Read one message** → `mail.cmd.read` with `{ "uid": 123 }` (add `"max": 4000`
  to cap the body). Returns `from/to/subject/date/text/truncated/attachments/
  calendarEvents`. Summarize: 1–3 sentences plus any action item / deadline /
  meeting time. If `calendarEvents` is non-empty it's an invite — give title,
  start/end, organizer, location.

Read only what the request needs. Get a `uid` from `list` before `read`/`reply`.

## Sending (ALWAYS confirm with the user first)

Show the draft (to / subject / body) and get an explicit yes before sending.

- **New email** → `mail.cmd.send` with `{ "to": "person@example.com",
  "subject": "…", "text": "…" }`.
- **Reply** → first `mail.cmd.read` the original to get its `from`/`replyTo`,
  `subject`, and `messageId`/`references`, then `mail.cmd.reply` with
  `{ "to": "<reply address>", "subject": "<original subject>", "text": "…",
  "in_reply_to": "<messageId>", "references": "<references or messageId>" }`.
  The connector adds the `Re:` prefix and threads the reply. Pass `in_reply_to`
  so it lands in the right thread.

## Errors

If a command returns `{ "error": … }`, surface the short message (e.g. an auth
or connection problem) — don't invent success. Never expose mail passwords.
