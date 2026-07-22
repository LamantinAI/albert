//! Bridge to `octo-history`: re-exports the neutral history types and adds the
//! only LLM-specific bit — converting stored [`Turn`]s into `rig` chat messages.
//! This is the **hot context** tier (the rolling per-channel transcript), distinct
//! from kaeru (deliberate memory).

pub use octo_history::{FileHistory, HistoryStore, InMemoryHistory, Role, SqliteHistory, Turn};

use rig::completion::Message;

/// Convert stored turns into the `rig` history the model expects.
pub fn to_messages(turns: &[Turn]) -> Vec<Message> {
    turns
        .iter()
        .map(|t| match t.role {
            Role::User => Message::user(t.content.clone()),
            Role::Assistant => Message::assistant(t.content.clone()),
        })
        .collect()
}
