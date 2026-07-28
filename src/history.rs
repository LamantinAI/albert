//! Bridge to `octo-history`: re-exports the neutral history types and adds the
//! only LLM-specific bit — converting stored [`Turn`]s into `rig` chat messages.
//! This is the **hot context** tier (the rolling per-channel transcript), distinct
//! from kaeru (deliberate memory).

pub use octo_history::{FileHistory, HistoryStore, InMemoryHistory, Role, SqliteHistory, Turn};

use rig::completion::Message;

/// Delimiter the cogitator appends before a turn's action log when persisting it
/// (see `cogitator::with_action_log`). Everything from here to the end of an
/// assistant turn's stored content is the system's record of what the agent DID —
/// it must never re-enter the model as part of the agent's own message (or the
/// model echoes it into chat), so [`to_messages`] strips it and [`recent_actions`]
/// surfaces it separately, as preamble context.
pub const ACTION_MARKER: &str = "\n\n[actions taken this turn]";

/// Convert stored turns into the `rig` history the model expects. Assistant turns
/// are reduced to their *spoken* text — the appended action log is removed so the
/// model never sees it as part of its own reply (see [`ACTION_MARKER`]).
pub fn to_messages(turns: &[Turn]) -> Vec<Message> {
    turns
        .iter()
        .map(|t| match t.role {
            Role::User => Message::user(t.content.clone()),
            Role::Assistant => Message::assistant(spoken(&t.content).to_string()),
        })
        .collect()
}

/// An assistant turn's reply text with any appended action log stripped off.
fn spoken(content: &str) -> &str {
    match content.split_once(ACTION_MARKER) {
        Some((said, _actions)) => said.trim_end(),
        None => content,
    }
}

/// The action logs of the most recent turns (up to `max_turns` that recorded any),
/// in chronological order (newest last), for injection into the preamble as system
/// context. `None` when no recent turn did anything tool-worthy. This is the agent's
/// action memory — what it *did* — kept out of the transcript proper so it can't be
/// echoed back into chat.
pub fn recent_actions(turns: &[Turn], max_turns: usize) -> Option<String> {
    let mut blocks: Vec<&str> = Vec::new();
    for turn in turns.iter().rev() {
        if blocks.len() >= max_turns {
            break;
        }
        if let Some((_, actions)) = turn.content.split_once(ACTION_MARKER) {
            let actions = actions.trim();
            if !actions.is_empty() {
                blocks.push(actions);
            }
        }
    }
    if blocks.is_empty() {
        return None;
    }
    blocks.reverse(); // chronological, newest last
    Some(blocks.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assistant(content: &str) -> Turn {
        Turn { role: Role::Assistant, content: content.to_string() }
    }

    #[test]
    fn to_messages_strips_the_action_log_from_assistant_turns() {
        let turns = vec![assistant("Done, sir.\n\n[actions taken this turn]\n- restart {\"target\":\"process\"} -> ok")];
        let msgs = to_messages(&turns);
        // The reconstructed assistant message is the spoken text only.
        let rendered = format!("{:?}", msgs[0]);
        assert!(rendered.contains("Done, sir."), "{rendered}");
        assert!(!rendered.contains("actions taken this turn"), "{rendered}");
        assert!(!rendered.contains("restart"), "{rendered}");
    }

    #[test]
    fn recent_actions_collects_newest_last_bounded() {
        let turns = vec![
            assistant("a\n\n[actions taken this turn]\n- one -> ok"),
            assistant("b (no tools)"),
            assistant("c\n\n[actions taken this turn]\n- two -> ok"),
            assistant("d\n\n[actions taken this turn]\n- three -> ok"),
        ];
        let out = recent_actions(&turns, 2).unwrap();
        // Only the last two action-bearing turns, chronological.
        assert_eq!(out, "- two -> ok\n- three -> ok");
    }

    #[test]
    fn recent_actions_none_when_nothing_done() {
        let turns = vec![assistant("just talking")];
        assert!(recent_actions(&turns, 3).is_none());
    }
}
