---
name: codeinside-jira
description: when the user asks about their CodeInside Jira (jira.codeinside.ru) — issues, tasks, boards, statuses, assignees, comments, worklogs, or asks to update/comment/transition/assign a ticket. Triggers «что в жире», «мои задачи», «покажи задачу RAGSYSTEM-…», «прокомментируй тикет», «переведи в In Progress», «залогируй время».
---

Work with the CodeInside Jira through the **jira** connector — reach it with the
`dispatch_to_connector` tool (target `"jira"`, an `octo.*`-style command kind, and a
JSON payload). It is an on-prem Jira Server, REST API v2. Every command returns the
raw Jira JSON; read what you need and summarize in short Russian unless asked otherwise.

## Reading (safe — do freely)

- **My open tasks / «что в жире» / «мои задачи»** → `jira.cmd.search` with
  `{ "jql": "assignee = currentUser() AND resolution = Unresolved ORDER BY updated DESC", "max_results": 10 }`.
  Summarize each issue: key, status, assignee, summary, updated. `max_results` is
  REQUIRED (a number) — default 10.
- **Any filter / search** → `jira.cmd.search` with your own JQL and `max_results`.
- **One issue in detail** (`RAGSYSTEM-802` etc.) → `jira.cmd.issue` with `{ "key": "RAGSYSTEM-802" }`.
  Summarize status, assignee, reporter, priority, labels, description, and the latest
  comments (`fields.comment.comments`, last few).
- **Auth check** → `jira.cmd.myself` with `{}`.

Read only what the request needs; don't dump whole issue bodies unless asked.

## Writing (only when the user explicitly asks)

State what will change before doing it (except a plainly-requested comment).

- **Comment** → `jira.cmd.comment` with `{ "key": "RAGSYSTEM-802", "body": "текст" }`.
- **Transition** — two steps: first `jira.cmd.transitions` with `{ "key": "…" }` to get
  the available transitions, pick the one whose `name` or `to.name` matches what the
  user wants, then `jira.cmd.transition` with `{ "key": "…", "transition_id": "<id>" }`.
- **Log work** → `jira.cmd.worklog` with `{ "key": "…", "time": "2h 30m", "comment": "что делал" }`.
  `time` accepts `2h`, `30m`, `1h 30m`. It stamps "now"; only pass a past time if asked.
- **Reassign** → `jira.cmd.assign` with `{ "key": "…", "user": "<username>" }`.

## Errors

If a command comes back as `jira.event.error`, read `details`:
- `status: 401/403` with a `x-authentication-denied-reason` (CAPTCHA) or
  `x-seraph-loginreason` header → the token is blocked or 2FA/CAPTCHA is in the way.
  Tell the user to log in once at https://jira.codeinside.ru/login.jsp to clear the
  challenge, or that the Personal Access Token needs renewing.
- Other errors → surface the short message; don't invent success.
