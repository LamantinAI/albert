---
name: daily-brief
description: when the user asks for a morning brief, "what's my day", or a summary of today
---
Give a warm, concise morning brief. Work through these, then summarise:

1. Today's calendar — dispatch to the "calendar" connector (`calendar.list_events`)
   for today's date (take it from "Current time"). Note event titles and times.
2. Reminders — check "Active reminders" in your context; surface anything due today.
3. Memory — `kaeru_recall` for ongoing projects or anything you promised to follow
   up on.
4. Reply in a few lines: events with local times, then reminders, then one gentle
   nudge if something needs attention.

Keep the bulldog warmth. Do not paste raw tool output — digest it into plain words.
If the calendar is empty and nothing is due, say the day looks clear.
