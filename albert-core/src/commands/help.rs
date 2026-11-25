//! Human-facing command help, separate from command parsing.
use super::{SkillCommand, SYSTEM};
/// A skill's description cut to one short line for `/help` and the menu.
pub fn about(description: &str) -> String {
    let normalized = description.split_whitespace().collect::<Vec<_>>().join(" ");
    let first = normalized
        .split(". ")
        .next()
        .unwrap_or(&normalized)
        .trim()
        .trim_end_matches('.');
    let mut line: String = first.chars().take(80).collect();
    if first.chars().count() > 80 {
        line.push('…');
    }
    line
}
/// A compact command reference filtered by the caller's permissions.
pub fn help(skills: &[SkillCommand], owner: bool, acl_admin: bool) -> String {
    let mut out = String::from("**Commands**\n");
    for (name, what, owner_only) in SYSTEM {
        if !owner_only {
            out.push_str(&format!("\n- /{name} — {what}"));
        }
    }
    let usable: Vec<_> = skills.iter().filter(|c| owner || !c.owner).collect();
    if !usable.is_empty() {
        out.push_str("\n\n**Skills**\n");
        for c in usable {
            out.push_str(&format!(
                "\n- /{} — {}",
                c.name,
                help_text(&about(&c.about))
            ));
        }
    }
    let management: Vec<_> = SYSTEM
        .into_iter()
        .filter(|(name, _, owner_only)| {
            *owner_only && (owner || (acl_admin && matches!(*name, "allow" | "deny" | "allowed")))
        })
        .collect();
    if !management.is_empty() {
        out.push_str(if owner {
            "\n\n**Owner commands**\n"
        } else {
            "\n\n**Access management**\n"
        });
        for (name, what, _) in management {
            out.push_str(&format!("\n- /{name} — {what}"));
        }
    }
    out
}
/// Account-mode transports keep legacy ACL controls out of their command help.
pub fn account_help(skills: &[SkillCommand], owner: bool, private: bool) -> String {
    let mut out = help(skills, owner, false)
        .lines()
        .filter(|line| {
            let name = line
                .strip_prefix("- /")
                .and_then(|value| value.split_whitespace().next());
            !matches!(name, Some("allow" | "deny" | "allowed" | "groupmode"))
                && (private
                    || !matches!(
                        name,
                        Some(
                            "sessions"
                                | "new"
                                | "fork"
                                | "open"
                                | "rename"
                                | "tmp"
                                | "save"
                                | "end"
                        )
                    ))
        })
        .collect::<Vec<_>>()
        .join("\n");
    if private {
        if !owner {
            out.push_str(
                "\n- /cancel — Stop the current task\n- /compact — Compact this conversation",
            );
        }
        out.push_str("\n- /logout — Disconnect Telegram without deleting conversations");
    }
    out
}
/// Skill metadata should remain one readable line, not introduce chat markup.
pub(super) fn help_text(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if matches!(c, '\\' | '*' | '_' | '[' | ']' | '<' | '>') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}
