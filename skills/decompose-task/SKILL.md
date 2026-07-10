---
name: decompose-task
description: when the user gives a big or multi-step task to carry out over several turns
---
Turn a multi-step task into a verifiable plan on the scratchpad:

1. `scratchpad_goal` — one line stating the overall goal.
2. `scratchpad_step` — add each concrete, checkable step (small units of work).
3. As you work, `scratchpad_mark` a step `done`, then `verified` only once you have
   actually confirmed the result — not merely attempted it.
4. `scratchpad_note` — record findings you must not lose between turns.
5. The task is complete only when every step is `verified`. `scratchpad_clear` once
   the user confirms it's finished.

Keep the plan tight — do not over-decompose a trivial task into ceremony.
