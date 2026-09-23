//! Chat commands, in two kinds:
//!
//! - **System** commands are reflexes built into Albert: instant, no model, and they cut in
//!   even while a turn is running — `/cancel` and `/restart` (owner), `/help`, `/start`, and
//!   the owner's access commands. Their names are reserved.
//! - **Skill** commands come from a skill's `command:` field: `/name args` starts an ordinary
//!   agent turn seeded with that skill's instructions, and the agent carries the task out by
//!   them. `command_owner: true` keeps one to the owner.
//!
//! Parsing order in the cogitator: system command → skill command → ordinary text (an
//! unknown `/word` simply reaches the agent as a message).

use std::{sync::Arc, time::Duration};

use octo_core::{ConnectorId, Envelope, EventBus, EventKind, InProcessBus};
use serde_json::{json, Value};
use tokio::time::sleep;
use tracing::{info, warn};

/// Names no skill can claim.
pub const RESERVED: [&str; 8] = ["start", "help", "cancel", "restart", "allow", "deny", "allowed", "status"];

/// The channel command that sets the bot's menu (octo's telegram connector accepts it).
pub const SET_COMMANDS: &str = "chat.set_commands";

/// System commands as they appear in `/help` and the menu: `(name, what it does, owner-only)`.
const SYSTEM: [(&str, &str, bool); 6] = [
    ("help", "What I can do, and my commands", false),
    ("cancel", "Stop the reply in progress", true),
    ("restart", "Restart me", true),
    ("allow", "Give a chat access: /allow <chat_id>", true),
    ("deny", "Take a chat's access away: /deny <chat_id>", true),
    ("allowed", "List the chats with access", true),
];

/// A command a skill declares.
#[derive(Clone, Debug, PartialEq)]
pub struct SkillCommand {
    /// The command name, without the `/`.
    pub name: String,
    /// The skill it runs.
    pub skill: String,
    /// One line on what it does (from the skill's description).
    pub about: String,
    /// Only the owner may run it.
    pub owner: bool,
}

/// `/name args` as typed: the lower-cased name (a `@botname` suffix, as Telegram adds in
/// groups, dropped) and the rest of the line.
#[derive(Debug, PartialEq)]
pub struct Invocation<'a> {
    pub name: String,
    pub args: &'a str,
}

/// Read a command off a message, or `None` if it isn't one.
pub fn parse(text: &str) -> Option<Invocation<'_>> {
    let rest = text.trim().strip_prefix('/')?;
    let (head, args) = match rest.split_once(char::is_whitespace) {
        Some((head, args)) => (head, args.trim()),
        None => (rest, ""),
    };
    let name = head.split('@').next()?.to_ascii_lowercase();
    valid_name(&name).then_some(Invocation { name, args })
}

/// A command name, channel-neutral: 1-64 of lower-case letters, digits, `_` and `-`.
/// Whatever stricter rule a channel has (Telegram: 1-32 of `a-z0-9_`) is the channel's to
/// apply to its own menu; the command still works as typed text.
pub fn valid_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// Why a skill's `command:` can't be registered, if it can't.
pub fn refusal(name: &str) -> Option<&'static str> {
    if !valid_name(name) {
        Some("a command is 1-64 of lower-case letters, digits, _ and -")
    } else if RESERVED.contains(&name) {
        Some("the name is reserved for a system command")
    } else {
        None
    }
}

/// A skill's description cut to one short line for `/help` and the menu.
pub fn about(description: &str) -> String {
    let first = description.split(". ").next().unwrap_or(description).trim().trim_end_matches('.');
    let mut line: String = first.chars().take(100).collect();
    if first.chars().count() > 100 {
        line.push('…');
    }
    line
}

/// The `/help` reply: what Albert does, then the system and skill commands this user may
/// run (the owner sees the owner-only ones too).
pub fn help(skills: &[SkillCommand], owner: bool) -> String {
    let mut out = String::from(
        "I keep context and act: reminders and calendar, voice messages (and answers in voice), \
         pictures, files, web search, and skills. Just write — or use a command.\n\nSystem commands:",
    );
    for (name, what, owner_only) in SYSTEM {
        if owner || !owner_only {
            out.push_str(&format!("\n/{name} — {what}"));
        }
    }
    let usable: Vec<&SkillCommand> = skills.iter().filter(|c| owner || !c.owner).collect();
    if !usable.is_empty() {
        out.push_str("\n\nSkill commands:");
        for c in usable {
            out.push_str(&format!("\n/{} — {}", c.name, c.about));
        }
    }
    out
}

/// The bot menu for `chat.set_commands`: everyone's commands, and the owner-only ones.
pub fn menu(skills: &[SkillCommand]) -> Value {
    let entry = |name: &str, about: &str| json!({ "command": name, "description": about });
    let (mut common, mut owner) = (Vec::new(), Vec::new());
    for (name, what, owner_only) in SYSTEM {
        if owner_only { &mut owner } else { &mut common }.push(entry(name, what));
    }
    for c in skills {
        if c.owner { &mut owner } else { &mut common }.push(entry(&c.name, &c.about));
    }
    json!({ "commands": common, "owner_commands": owner })
}

/// The prompt a skill command's turn starts from: what was run, then the skill's own
/// instructions (and its bundled files), so the agent carries the task out by them.
pub fn seed(command: &SkillCommand, args: &str, instructions: &str, files: &[String]) -> String {
    let asked = if args.is_empty() { String::new() } else { format!(" with: {args}") };
    let mut out = format!(
        "The user ran the /{} command{asked}. It runs the `{}` skill: carry its task out now, \
         following the skill's instructions below (they are already loaded — no need to \
         skill_apply it), and use what the user gave with the command, if anything.\n\n\
         --- instructions of the `{}` skill ---\n{instructions}",
        command.name, command.skill, command.skill
    );
    if !files.is_empty() {
        out.push_str(&format!("\n\n(bundled files of the skill: {})", files.join(", ")));
    }
    out
}

/// Set the bot menu on every connector that accepts `chat.set_commands`. Retries a few times
/// while the channels come up; runs detached so it never blocks the cogitator.
pub async fn publish_menu(bus: Arc<InProcessBus>, source: ConnectorId, targets: Vec<ConnectorId>, menu: Value) {
    for target in targets {
        let mut done = false;
        for _ in 0..6u32 {
            sleep(Duration::from_millis(500)).await;
            let env = Envelope::new(source.clone(), EventKind::from_static(SET_COMMANDS), menu.clone())
                .with_target(target.clone());
            if let Ok(resp) = bus.publish_and_await_response(env, Duration::from_secs(10)).await {
                let result = resp.payload_as::<Value>().cloned().unwrap_or(Value::Null);
                info!(connector = %target, result = %result, "commands: menu published");
                done = true;
                break;
            }
        }
        if !done {
            warn!(connector = %target, "commands: could not publish the menu (channel not reachable)");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{help, menu, parse, refusal, seed, Invocation, SkillCommand};

    fn cmd(name: &str, owner: bool) -> SkillCommand {
        SkillCommand { name: name.into(), skill: format!("{name}-skill"), about: format!("does {name}"), owner }
    }

    #[test]
    fn commands_are_read_off_a_message() {
        assert_eq!(parse("/brief"), Some(Invocation { name: "brief".into(), args: "" }));
        assert_eq!(parse("  /Draw  a red fox "), Some(Invocation { name: "draw".into(), args: "a red fox" }));
        assert_eq!(parse("/brief@albert_bot tomorrow"), Some(Invocation { name: "brief".into(), args: "tomorrow" }));
        assert_eq!(parse("hello /brief"), None);
        assert_eq!(parse("/not-a-command"), Some(Invocation { name: "not-a-command".into(), args: "" }));
        assert_eq!(parse("/"), None);
    }

    #[test]
    fn reserved_and_malformed_names_are_refused() {
        assert!(refusal("cancel").is_some());
        assert!(refusal("Brief").is_some());
        assert!(refusal("brief").is_none());
    }

    #[test]
    fn help_and_menu_split_by_owner() {
        let skills = [cmd("brief", false), cmd("settings", true)];
        let guest = help(&skills, false);
        assert!(guest.contains("/brief") && !guest.contains("/settings") && !guest.contains("/restart"));
        let owner = help(&skills, true);
        assert!(owner.contains("/settings") && owner.contains("/restart"));
        let m = menu(&skills);
        let names = |key: &str| m[key].as_array().unwrap().iter().map(|e| e["command"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!(names("commands"), ["help", "brief"]);
        assert!(names("owner_commands").contains(&"settings".to_string()));
    }

    #[test]
    fn a_seed_names_the_command_the_args_and_the_instructions() {
        let s = seed(&cmd("draw", false), "a red fox", "Build the spec, then call imagegen.run.", &[]);
        assert!(s.contains("/draw command with: a red fox"));
        assert!(s.contains("Build the spec"));
    }
}
