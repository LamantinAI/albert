use crate::subagents::{Limits, SubagentTool, Task};
pub(super) fn budget(
    tool: &SubagentTool,
    task: &Task,
    limits: &Limits,
) -> Result<(usize, u64), String> {
    if tool.ctx.shutdown.is_cancelled() {
        return Err("Runtime is shutting down.".into());
    }
    if tool
        .session
        .as_ref()
        .is_some_and(|session| !session.is_live())
    {
        return Err("Session ended; child was not started.".into());
    }
    if tool
        .session
        .as_ref()
        .is_some_and(|session| session.chat_only())
    {
        return Err("This account cannot delegate work.".into());
    }
    if task.task.trim().is_empty() {
        return Err("Subagent task is empty.".into());
    }
    let max_turns = task
        .max_tool_turns
        .unwrap_or(limits.default_tool_turns.min(limits.max_tool_turns));
    let seconds = task.timeout_secs.unwrap_or(limits.timeout_secs);
    if max_turns == 0
        || max_turns > limits.max_tool_turns
        || seconds == 0
        || seconds > limits.timeout_secs
    {
        return Err("Requested child budget exceeds configured limits.".into());
    }

    Ok((max_turns, seconds))
}
