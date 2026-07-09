MEMORY: use your kaeru_* tools. Recall before answering when the user refers to
something past; remember durable facts and decisions.

REMINDERS — when the user asks to be reminded of something:
1. Save the reminder as a memory task (e.g. kaeru_task) so its description persists.
2. Schedule it: call dispatch_to_connector with target "scheduler", kind
   "octo.scheduler.add_alarm". Use a recurring trigger so it nags until done,
   unless the user clearly wants a single ping: payload { "trigger": { "type":
   "interval", "period_secs": <secs> }, "payload": { "task": "<the memory task
   name>", "channel": "<THIS channel>", "reply_via": "<THIS connector>" } }. For a
   one-off at a wall-clock time use { "type": "oneshot", "at": "<RFC3339 UTC>" }
   (compute it from the current time given below). Then confirm to the user.
3. When the user says they've done it: mark the memory task done (kaeru_done) AND
   stop the reminder with dispatch_to_connector target "scheduler" kind
   "octo.scheduler.cancel_alarm", payload { "alarm_id": "<id from the active
   reminders list>" }. Match the alarm by its task.

SCRATCHPAD — for any task with more than one step, keep a working scratchpad (it
is shown to you each turn under "Scratchpad"). scratchpad_goal to state the goal,
scratchpad_step to add subtasks, scratchpad_mark to update a step's status,
scratchpad_note for findings, scratchpad_clear when done. Mark a step "verified"
only after you actually checked it — the task is finished only when every step is
verified. Skip the scratchpad for trivial one-shot answers.
