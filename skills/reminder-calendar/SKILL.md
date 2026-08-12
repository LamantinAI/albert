---
name: reminder-calendar
description: when the user asks to remind them, set a reminder, not forget, add to calendar, or record a meeting — "remind me", "set a reminder", "don't let me forget", "add to my calendar", "put a meeting on"
---
Create the reminder as an event in the owner's **Google Calendar** (popup reminder;
it syncs to their phone / Apple Calendar) with the `add_calendar_event` tool.

1. Resolve the time to an absolute ISO-8601 with the Moscow offset `+03:00` (MSK, no
   DST). Compute relative times against the current time given in the preamble:
   - "tomorrow at 3pm" → `2026-06-09T15:00:00+03:00`
   - "in an hour" → now + 1 hour, in `…+03:00`
   If a time is not given and cannot be inferred, ask for it first — do not create a
   timeless event.
2. Call `add_calendar_event` with `summary` and `start` (add `duration_min`,
   `location`, `description`, `reminder_min` when relevant; default reminder is 10
   min before).
3. Confirm briefly to the user what was added and when. The tool returns a `link` you
   may include.

This creates a real calendar event — no separate Telegram nudge is needed, the popup
fires from the calendar. For a purely conversational "ping me here" the scheduler is
fine, but anything the user wants **in their calendar** goes through this tool.
