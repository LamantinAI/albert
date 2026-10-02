//! Declarative skills: metadata stays visible; instructions load on demand.
//! At startup we scan `skills/*/SKILL.md` for each skill's name and description.
//! Every available skill's metadata is included in the preamble regardless of
//! catalog size. `skill_list` pagination never changes this discovery surface.
//! `skill_apply` loads a body on demand into an LRU cache (`[skills] cache`).
//! The agent finds skills three ways: the inline catalog, `skill_search <query>`
//! (ranked by name/description match), and `skill_list <page>` (paginated browse);
//! then `skill_apply <name>`.
//!
//! A `SKILL.md`:
//! ```text
//! ---
//! name: daily-brief
//! description: when the user asks for a morning brief / "what's my day"
//! ---
//! <instructions the agent follows once this skill is applied>
//! ```
//! Layout: `skills/<name>/SKILL.md` + any bundled resources (templates, references,
//! example files) in the same folder. `skill_apply` returns the instructions plus a
//! list of the bundled files; `skill_file` reads one **in place** (read-only, jailed
//! to the skill's own folder). The agent reads a skill's resources where they live
//! and does its actual work in the separate octo-code workspace — it never copies the
//! skill into the workspace.

use std::{
    collections::VecDeque,
    fs::read_to_string,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
};

use serde_json::{json, Value};
use tracing::{debug, info};

use crate::commands::SkillCommand;

mod discovery;

mod tools;

use self::{
    discovery::{body_of, bundle_files, scan, skill_command},
    tools::{SkillApply, SkillFile, SkillList, SkillSearch},
};

/// Default number of matches `skill_search` returns (clamped 1..=`SEARCH_MAX`).
const SEARCH_LIMIT: usize = 10;
const SEARCH_MAX: usize = 25;

/// One skill's catalog entry (parsed frontmatter) + its folder.
struct SkillMeta {
    name: String,
    when: String,
    /// For an `always: true` skill, its instructions — read once at scan time and
    /// held in memory, since they go into every single turn.
    standing: Option<String>,
    /// The skill's own folder (`skills/<name>/`) — holds `SKILL.md` and any bundled
    /// resources. `skill_file` reads are jailed to it.
    dir: PathBuf,
    /// The chat command that runs it (`command:`), if it declares a valid, free one.
    command: Option<String>,
    /// `command_owner: true` — only the owner may run that command.
    command_owner: bool,
    /// `command_about:` — the command's one line for `/help` and the menu.
    command_about: Option<String>,
}

struct Inner {
    catalog: Vec<SkillMeta>,
    /// LRU read-through cache of loaded `(name, body)`; front = most-recently applied.
    cache: VecDeque<(String, String)>,
    cache_cap: usize,
    /// `skill_list` page size only; never limits the preamble catalog.
    page: usize,
}

/// Catalog of installed skills + an LRU cache of applied skill bodies.
pub struct SkillStore {
    inner: Mutex<Inner>,
}

impl SkillStore {
    /// Scan `dir` for `*/SKILL.md` and build the catalog. A missing `dir` yields an
    /// empty catalog (skills are optional).
    ///
    /// `capabilities` are what this runtime can actually do (e.g. `subscription`).
    /// A skill declaring `requires: <cap>` for a capability that's absent is left out
    /// of the catalog entirely, so the agent never offers what it can't deliver.
    pub fn load(dir: PathBuf, cache_cap: usize, page: usize, capabilities: &[&str]) -> Arc<Self> {
        let catalog = scan(&dir, capabilities);
        info!(skills = catalog.len(), dir = %dir.display(), "skills: catalog loaded");
        Arc::new(Self {
            inner: Mutex::new(Inner {
                catalog,
                cache: VecDeque::new(),
                cache_cap: cache_cap.max(1),
                page: page.max(1),
            }),
        })
    }

    /// Every selectable skill's name and complete trigger description, on every
    /// turn. Bodies stay out until `skill_apply`, except for standing instructions.
    pub fn catalog(&self) -> String {
        let inner = self.inner.lock().unwrap();
        // An `always: true` skill is not offered, it is IN FORCE: its body goes in
        // verbatim every turn, so obeying it costs no tool call and it cannot lapse
        // between turns. It is also never paginated away — a standing instruction
        // that disappears once the catalog grows would be worse than none.
        let mut out = String::new();
        for skill in inner.catalog.iter() {
            let Some(body) = &skill.standing else {
                continue;
            };
            out.push_str(&format!(
                "STANDING INSTRUCTIONS — \"{}\", in force for EVERY reply, not optional and not \
                 expiring between turns. They shape HOW you answer; your persona (who you are, \
                 your voice) still governs. Where the two collide, keep the voice and follow the \
                 shape.\n\n{body}\n\n---\n\n",
                skill.name,
            ));
        }
        // Only the selectable skills are listed or counted — an in-force one has
        // already been applied.
        let listable: Vec<&SkillMeta> = inner
            .catalog
            .iter()
            .filter(|s| s.standing.is_none())
            .collect();
        let n = listable.len();
        if n == 0 {
            out.push_str("Skills: (none installed).");
        } else {
            out.push_str(&format!(
                "Skills available ({n} installed; apply a matching skill with skill_apply; \
                 skill_search to search, skill_list to browse by page):\n"
            ));
            for skill in listable {
                out.push_str(&format!("- {}: {}\n", skill.name, skill.when));
            }
        }
        out
    }

    /// One page of the catalog (name + when-to-use), 1-indexed, `PAGE` per page.
    fn list_json(&self, page: usize) -> Value {
        let inner = self.inner.lock().unwrap();
        let size = inner.page;
        let total = inner.catalog.len();
        let pages = total.div_ceil(size).max(1);
        let page = page.clamp(1, pages);
        let items: Vec<Value> = inner
            .catalog
            .iter()
            .skip((page - 1) * size)
            .take(size)
            .map(|s| json!({ "name": s.name, "when": s.when }))
            .collect();
        json!({ "skills": items, "page": page, "pages": pages, "total": total })
    }

    /// Rank the catalog against `query` (keywords). A term in a skill's name scores 2,
    /// in its description 1; skills with any hit are returned best-first, up to `limit`.
    /// An empty query just returns the first `limit` by name (a browse fallback).
    fn search_json(&self, query: &str, limit: usize) -> Value {
        let inner = self.inner.lock().unwrap();
        let q = query.to_lowercase();
        let terms: Vec<&str> = q.split_whitespace().collect();

        let mut scored: Vec<(i32, &SkillMeta)> = if terms.is_empty() {
            inner.catalog.iter().map(|s| (0, s)).collect()
        } else {
            inner
                .catalog
                .iter()
                .filter_map(|s| {
                    let name = s.name.to_lowercase();
                    let when = s.when.to_lowercase();
                    let score: i32 = terms
                        .iter()
                        .map(|t| (name.contains(t) as i32) * 2 + (when.contains(t) as i32))
                        .sum();
                    (score > 0).then_some((score, s))
                })
                .collect()
        };
        // Best score first; ties by name for a stable order.
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));
        let total_matched = scored.len();
        let items: Vec<Value> = scored
            .iter()
            .take(limit)
            .map(|(_, s)| json!({ "name": s.name, "when": s.when }))
            .collect();
        json!({
            "query": query,
            "total_matched": total_matched,
            "returned": items.len(),
            "skills": items,
        })
    }

    fn apply_json(&self, name: &str) -> Value {
        let mut inner = self.inner.lock().unwrap();
        let Some(dir) = inner
            .catalog
            .iter()
            .find(|s| s.name == name)
            .map(|s| s.dir.clone())
        else {
            return json!({ "ok": false, "error": format!("no skill '{name}'") });
        };
        let files = bundle_files(&dir);
        debug!(skill = name, files = files.len(), "skill apply");
        // Cache hit: return the warm body and move it to the front — no disk read.
        if let Some(pos) = inner.cache.iter().position(|(n, _)| n == name) {
            let entry = inner.cache.remove(pos).expect("position just found");
            let body = entry.1.clone();
            inner.cache.push_front(entry);
            return json!({ "ok": true, "applied": name, "cached": true, "instructions": body, "files": files });
        }
        // Miss: read SKILL.md, cache the body (LRU, evict the back).
        let body = match read_to_string(dir.join("SKILL.md")) {
            Ok(text) => body_of(&text).trim().to_string(),
            Err(e) => return json!({ "ok": false, "error": format!("read {name}: {e}") }),
        };
        inner.cache.push_front((name.to_string(), body.clone()));
        let cap = inner.cache_cap;
        while inner.cache.len() > cap {
            inner.cache.pop_back();
        }
        json!({ "ok": true, "applied": name, "instructions": body, "files": files })
    }

    /// Read one of a skill's bundled files in place (read-only, jailed to the skill's
    /// own folder). For informational resources (templates / references) — bytes for
    /// scripts / fonts are a separate concern (see the module note).
    fn file_json(&self, name: &str, rel: &str) -> Value {
        let inner = self.inner.lock().unwrap();
        let Some(dir) = inner
            .catalog
            .iter()
            .find(|s| s.name == name)
            .map(|s| s.dir.clone())
        else {
            return json!({ "ok": false, "error": format!("no skill '{name}'") });
        };
        let rp = Path::new(rel);
        if rel.is_empty()
            || rp.is_absolute()
            || rp.components().any(|c| matches!(c, Component::ParentDir))
        {
            return json!({ "ok": false, "error": "path must be relative and stay within the skill" });
        }
        debug!(skill = name, path = rel, "skill file read");
        match read_to_string(dir.join(rp)) {
            Ok(content) => json!({ "ok": true, "name": name, "path": rel, "content": content }),
            Err(e) => json!({ "ok": false, "error": format!("read {name}/{rel}: {e}") }),
        }
    }

    pub fn list_tool(self: &Arc<Self>) -> SkillList {
        SkillList(Arc::clone(self))
    }
    pub fn search_tool(self: &Arc<Self>) -> SkillSearch {
        SkillSearch(Arc::clone(self))
    }
    pub fn apply_tool(self: &Arc<Self>) -> SkillApply {
        SkillApply(Arc::clone(self))
    }
    pub fn file_tool(self: &Arc<Self>) -> SkillFile {
        SkillFile(Arc::clone(self))
    }

    /// Every chat command the skills declare, by command name.
    pub fn commands(&self) -> Vec<SkillCommand> {
        let inner = self.inner.lock().unwrap();
        inner.catalog.iter().filter_map(skill_command).collect()
    }

    /// The skill command called `name`, if a skill declares it.
    pub fn command(&self, name: &str) -> Option<SkillCommand> {
        let inner = self.inner.lock().unwrap();
        inner
            .catalog
            .iter()
            .filter_map(skill_command)
            .find(|c| c.name == name)
    }

    /// A skill's instructions and bundled files, loaded as `skill_apply` would (through
    /// the same LRU cache).
    pub fn instructions(&self, skill: &str) -> Result<(String, Vec<String>), String> {
        let v = self.apply_json(skill);
        match v.get("instructions").and_then(Value::as_str) {
            Some(body) => {
                let files = v["files"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_string);
                Ok((body.to_string(), files.collect()))
            }
            None => Err(v
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("skill not found")
                .to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        env::temp_dir,
        fs::{create_dir_all, remove_dir_all, write},
        path::PathBuf,
    };

    use tempfile::tempdir;

    use super::{discovery::meta_of, scan, SkillStore};

    /// A throwaway `skills/` tree; `skills` is a list of `(folder, SKILL.md body)`.
    fn skills_dir(tag: &str, skills: &[(&str, &str)]) -> PathBuf {
        let root = temp_dir().join(format!("albert-skills-test-{tag}"));
        let _ = remove_dir_all(&root);
        for (name, body) in skills {
            create_dir_all(root.join(name)).unwrap();
            write(root.join(name).join("SKILL.md"), body).unwrap();
        }
        root
    }

    #[test]
    fn adding_an_eleventh_skill_preserves_every_trigger_without_loading_bodies() {
        let dir = tempdir().unwrap();
        for count in 1..=35 {
            let name = format!("skill-{count:02}");
            let path = dir.path().join(&name);
            create_dir_all(&path).unwrap();
            write(path.join("SKILL.md"), format!(
                "---\nname: {name}\ndescription: Проверяй устройство device-{count:02}\n---\nPRIVATE_RECIPE_{count:02}"
            )).unwrap();
            if ![10, 11, 35].contains(&count) {
                continue;
            }
            let store = SkillStore::load(dir.path().to_path_buf(), 5, 10, &[]);
            let catalog = store.catalog();
            for index in 1..=count {
                assert!(
                    catalog.contains(&format!(
                        "- skill-{index:02}: Проверяй устройство device-{index:02}"
                    )),
                    "missing skill {index} with {count} installed: {catalog}"
                );
            }
            assert!(!catalog.contains("PRIVATE_RECIPE"));
            let first = store.list_json(1);
            assert_eq!(first["skills"].as_array().unwrap().len(), 10);
            assert_eq!(first["total"], count);
            if count > 10 {
                assert_eq!(store.list_json(2)["skills"][0]["name"], "skill-11");
                assert_eq!(
                    store.search_json("device-11", 10)["skills"][0]["name"],
                    "skill-11"
                );
                assert_eq!(
                    store.instructions("skill-11").unwrap().0,
                    "PRIVATE_RECIPE_11"
                );
            }
            assert_eq!(
                catalog,
                SkillStore::load(dir.path().to_path_buf(), 5, 1, &[]).catalog()
            );
        }
    }

    #[test]
    fn skills_declare_commands_reserved_and_duplicate_ones_are_refused() {
        let dir = skills_dir(
            "commands",
            &[
                ("brief", "---\nname: brief\ndescription: when asked for a brief\ncommand: brief\ncommand_about: Your day at a glance\n---\nDo the brief."),
                ("config", "---\nname: config\ndescription: change setup\ncommand: /settings\ncommand_owner: true\n---\nbody"),
                ("halt", "---\nname: halt\ndescription: d\ncommand: cancel\n---\nbody"),
                ("shout", "---\nname: shout\ndescription: d\ncommand: Loud-Name\n---\nbody"),
                ("zbrief", "---\nname: zbrief\ndescription: d\ncommand: brief\n---\nbody"),
            ],
        );
        let store = SkillStore::load(dir.clone(), 5, 10, &[]);
        let commands = store.commands();
        let names: Vec<(&str, &str, bool)> = commands
            .iter()
            .map(|c| (c.name.as_str(), c.skill.as_str(), c.owner))
            .collect();
        assert_eq!(
            names,
            [("brief", "brief", false), ("settings", "config", true)]
        );
        assert_eq!(commands[0].about, "Your day at a glance");
        assert_eq!(
            store.command("settings").map(|c| c.skill),
            Some("config".to_string())
        );
        assert_eq!(store.instructions("brief").unwrap().0, "Do the brief.");
        let _ = remove_dir_all(&dir);
    }

    #[test]
    fn always_is_parsed_from_frontmatter() {
        assert!(meta_of("---\nname: s\nalways: true\n---\nbody").always);
        assert!(!meta_of("---\nname: s\ndescription: d\n---\nbody").always);
    }

    #[test]
    fn an_always_skill_is_in_force_not_offered() {
        let dir = skills_dir(
            "always",
            &[
                ("style", "---\nname: style\nalways: true\ndescription: how to write\n---\nLead with the next action."),
                ("brief", "---\nname: brief\ndescription: pick me when asked\n---\nbody"),
            ],
        );
        let catalog = SkillStore::load(dir.clone(), 5, 10, &[]).catalog();

        // In force: the body itself is present, so no tool call is needed to obey it.
        assert!(
            catalog.contains("Lead with the next action."),
            "got:\n{catalog}"
        );
        assert!(catalog.contains("STANDING INSTRUCTIONS"), "got:\n{catalog}");
        // Not offered: absent from the pick-one list, which still holds the others.
        assert!(
            !catalog.contains("- style:"),
            "an in-force skill must not be offered:\n{catalog}"
        );
        assert!(
            catalog.contains("- brief: pick me when asked"),
            "got:\n{catalog}"
        );

        let _ = remove_dir_all(&dir);
    }

    #[test]
    fn standing_instructions_and_skill_triggers_survive_small_list_pages() {
        // Small tool pages must hide neither standing instructions nor selectable skills.
        let dir = skills_dir(
            "always-paged",
            &[
                (
                    "style",
                    "---\nname: style\nalways: true\n---\nAction first.",
                ),
                ("a", "---\nname: a\ndescription: x\n---\nb"),
                ("b", "---\nname: b\ndescription: y\n---\nb"),
            ],
        );
        let catalog = SkillStore::load(dir.clone(), 5, 1, &[]).catalog();
        assert!(catalog.contains("Action first."), "got:\n{catalog}");
        assert!(
            catalog.contains("2 installed"),
            "in-force skills aren't counted:\n{catalog}"
        );
        assert!(catalog.contains("- a: x"));
        assert!(catalog.contains("- b: y"));
        let _ = remove_dir_all(&dir);
    }

    #[test]
    fn a_skill_is_hidden_until_its_capability_is_present() {
        let dir = skills_dir(
            "gating",
            &[
                (
                    "transcribe",
                    "---\nname: transcribe\nrequires: subscription\n---\nbody",
                ),
                (
                    "brief",
                    "---\nname: brief\ndescription: always here\n---\nbody",
                ),
            ],
        );
        let catalog = SkillStore::load(dir.clone(), 5, 1, &[]).catalog();
        assert!(catalog.contains("- brief: always here"));
        assert!(!catalog.contains("transcribe"));
        let without: Vec<String> = scan(&dir, &[]).into_iter().map(|s| s.name).collect();
        assert_eq!(
            without,
            vec!["brief"],
            "no subscription -> transcribe is absent"
        );
        let with: Vec<String> = scan(&dir, &["subscription"])
            .into_iter()
            .map(|s| s.name)
            .collect();
        assert_eq!(
            with,
            vec!["brief", "transcribe"],
            "subscription -> both, sorted"
        );
        let _ = remove_dir_all(&dir);
    }
}
