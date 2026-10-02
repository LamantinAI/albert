//! Read skill metadata, commands and bundled resources from disk.

use std::{
    collections::HashSet,
    fs::{read_dir, read_to_string},
    path::Path,
};

use tracing::{debug, warn};

use super::SkillMeta;
use crate::commands::{about, refusal, SkillCommand};

pub(super) fn skill_command(s: &SkillMeta) -> Option<SkillCommand> {
    let name = s.command.clone()?;
    let about = s.command_about.clone().unwrap_or_else(|| about(&s.when));
    Some(SkillCommand {
        name,
        skill: s.name.clone(),
        about,
        owner: s.command_owner,
    })
}

/// Scan `skills/<name>/SKILL.md` into catalog entries, sorted by name.
pub(super) fn scan(dir: &Path, capabilities: &[&str]) -> Vec<SkillMeta> {
    let Ok(entries) = read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        let skill_md = p.join("SKILL.md");
        let Ok(text) = read_to_string(&skill_md) else {
            continue;
        };
        let front = meta_of(&text);
        let name = front.name.unwrap_or_else(|| {
            p.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });
        // A skill whose requirement isn't met must be invisible, not merely unusable —
        // else the agent keeps offering something it cannot do.
        if let Some(req) = front
            .requires
            .as_deref()
            .filter(|r| !capabilities.contains(r))
        {
            debug!(skill = %name, requires = %req, "skills: hidden (capability unavailable)");
            continue;
        }
        // An always-on skill's body is read once, here: it is in force from the first
        // turn, so it must not depend on the agent choosing to load it.
        let standing = front.always.then(|| body_of(&text).trim().to_string());
        let command = front
            .command
            .map(|c| c.trim().trim_start_matches('/').to_string())
            .filter(|c| !c.is_empty())
            .filter(|c| match refusal(c) {
                Some(why) => {
                    warn!(skill = %name, command = %c, why, "skills: command not registered");
                    false
                }
                None => true,
            });
        out.push(SkillMeta {
            name,
            when: front
                .description
                .unwrap_or_else(|| "(no description)".to_string()),
            standing,
            dir: p,
            command,
            command_owner: front.command_owner,
            command_about: front.command_about,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    // Two skills claiming one command: the first by name keeps it.
    let mut taken = HashSet::new();
    for s in &mut out {
        if let Some(c) = &s.command {
            if !taken.insert(c.clone()) {
                warn!(skill = %s.name, command = %c, "skills: command already taken by another skill; not registered");
                s.command = None;
            }
        }
    }
    out
}

/// Relative paths of a skill's bundled files (everything but `SKILL.md`), sorted.
pub(super) fn bundle_files(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    collect_files(dir, dir, &mut out);
    out.retain(|p| p != "SKILL.md");
    out.sort();
    out
}

fn collect_files(root: &Path, cur: &Path, out: &mut Vec<String>) {
    let Ok(entries) = read_dir(cur) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_files(root, &p, out);
        } else if let Ok(rel) = p.strip_prefix(root) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

/// Split off the `---`…`---` frontmatter; return `(frontmatter, body)`.
fn split_frontmatter(text: &str) -> (&str, &str) {
    let t = text.trim_start();
    if let Some(after) = t.strip_prefix("---\n") {
        if let Some(idx) = after.find("\n---") {
            return (&after[..idx], after[idx + 4..].trim_start());
        }
    }
    ("", t)
}

pub(super) fn body_of(text: &str) -> &str {
    split_frontmatter(text).1
}

/// Parse `name:` and `description:` out of the frontmatter.
pub(super) fn meta_of(text: &str) -> SkillFront {
    let (fm, _) = split_frontmatter(text);
    let lines: Vec<&str> = fm.lines().collect();
    let mut front = SkillFront::default();
    for (i, line) in lines.iter().enumerate() {
        if let Some(v) = line.strip_prefix("name:") {
            front.name = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("description:") {
            front.description = Some(field_value(v, &lines[i + 1..]));
        } else if let Some(v) = line.strip_prefix("requires:") {
            front.requires = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("always:") {
            front.always = v.trim().eq_ignore_ascii_case("true");
        } else if let Some(v) = line.strip_prefix("command:") {
            front.command = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("command_owner:") {
            front.command_owner = v.trim().eq_ignore_ascii_case("true");
        } else if let Some(v) = line.strip_prefix("command_about:") {
            front.command_about = Some(v.trim().to_string()).filter(|a| !a.is_empty());
        }
    }
    front
}

/// A skill's parsed frontmatter.
#[derive(Default)]
pub(super) struct SkillFront {
    name: Option<String>,
    description: Option<String>,
    /// A capability the runtime must have for the skill to exist at all (today:
    /// `subscription`). Absent → always available.
    requires: Option<String>,
    /// `always: true` — not a skill to reach for, a standing instruction. Its body
    /// rides in the preamble every turn instead of waiting for `skill_apply`.
    pub(super) always: bool,
    /// `command: <name>` — a chat command (`/<name> args`) that runs this skill.
    command: Option<String>,
    /// `command_owner: true` — that command is the owner's only.
    command_owner: bool,
    /// `command_about:` — one line on the command for `/help` and the menu.
    command_about: Option<String>,
}

/// A frontmatter value, following YAML block scalars (`>`, `>-`, `|`, `|-`) into the
/// indented lines beneath — that's how a long `description:` is usually written, and
/// reading only the marker line would leave the skill with no description at all.
fn field_value(rest_of_line: &str, following: &[&str]) -> String {
    let head = rest_of_line.trim();
    if !matches!(head, ">" | ">-" | ">+" | "|" | "|-" | "|+") {
        return head.to_string();
    }
    following
        .iter()
        .take_while(|l| l.trim().is_empty() || l.starts_with([' ', '\t']))
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}
