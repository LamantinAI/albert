//! Declarative skills — a `skills/` folder of `SKILL.md` files (instructions, no
//! scripts yet). At startup we scan the **catalog** (name + when-to-use, from each
//! file's frontmatter). Only the catalog — never the bodies — sits in front of the
//! agent, and it **scales**: a catalog up to one page (`[skills] page`, default 10) is
//! shown inline each turn; a larger one is replaced by a count + a pointer to
//! `skill_search`, so 100 skills don't cost 100 lines of context every turn. `skill_apply` returns a
//! skill's instructions on demand and caches the body in a small **LRU RAM
//! read-through cache** (default 5), so re-applying a recent skill is a fast RAM hit.
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
    convert::Infallible,
    fs::{read_dir, read_to_string},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
};

use rig::{completion::ToolDefinition, tool::Tool};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{debug, info};

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
}

struct Inner {
    catalog: Vec<SkillMeta>,
    /// LRU read-through cache of loaded `(name, body)`; front = most-recently applied.
    cache: VecDeque<(String, String)>,
    cache_cap: usize,
    /// `skill_list` page size, and the inline-catalog threshold (from `[skills] page`).
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

    /// The per-turn menu for the preamble. Small catalog → the full list (cheap);
    /// large catalog (> [`INLINE_MAX`]) → a count + a pointer to `skill_search`, so it
    /// never floods context. Either way the bodies stay out until `skill_apply`.
    pub fn catalog(&self) -> String {
        let inner = self.inner.lock().unwrap();
        // An `always: true` skill is not offered, it is IN FORCE: its body goes in
        // verbatim every turn, so obeying it costs no tool call and it cannot lapse
        // between turns. It is also never paginated away — a standing instruction
        // that disappears once the catalog grows would be worse than none.
        let mut out = String::new();
        for skill in inner.catalog.iter() {
            let Some(body) = &skill.standing else { continue };
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
        let listable: Vec<&SkillMeta> =
            inner.catalog.iter().filter(|s| s.standing.is_none()).collect();
        let n = listable.len();
        if n == 0 {
            out.push_str("Skills: (none installed).");
        } else if n <= inner.page {
            let lines: Vec<String> =
                listable.iter().map(|s| format!("- {}: {}", s.name, s.when)).collect();
            out.push_str(&format!(
                "Skills available (apply one with skill_apply when it fits the task; skill_list to \
                 re-list):\n{}",
                lines.join("\n")
            ));
        } else {
            out.push_str(&format!(
                "Skills: {n} installed — too many to list here. When a task might match one, call \
                 skill_search with a few keywords to find it (ranked by name/description), or \
                 skill_list to browse by page; then skill_apply the one that fits."
            ));
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
        let Some(dir) = inner.catalog.iter().find(|s| s.name == name).map(|s| s.dir.clone()) else {
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
        let Some(dir) = inner.catalog.iter().find(|s| s.name == name).map(|s| s.dir.clone()) else {
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
}

/// Scan `skills/<name>/SKILL.md` into catalog entries, sorted by name.
fn scan(dir: &Path, capabilities: &[&str]) -> Vec<SkillMeta> {
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
        let name = front
            .name
            .unwrap_or_else(|| p.file_name().unwrap_or_default().to_string_lossy().into_owned());
        // A skill whose requirement isn't met must be invisible, not merely unusable —
        // else the agent keeps offering something it cannot do.
        if let Some(req) = front.requires.as_deref().filter(|r| !capabilities.contains(r)) {
            debug!(skill = %name, requires = %req, "skills: hidden (capability unavailable)");
            continue;
        }
        // An always-on skill's body is read once, here: it is in force from the first
        // turn, so it must not depend on the agent choosing to load it.
        let standing = front.always.then(|| body_of(&text).trim().to_string());
        out.push(SkillMeta {
            name,
            when: front.description.unwrap_or_else(|| "(no description)".to_string()),
            standing,
            dir: p,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Relative paths of a skill's bundled files (everything but `SKILL.md`), sorted.
fn bundle_files(dir: &Path) -> Vec<String> {
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

fn body_of(text: &str) -> &str {
    split_frontmatter(text).1
}

/// Parse `name:` and `description:` out of the frontmatter.
fn meta_of(text: &str) -> SkillFront {
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
        }
    }
    front
}

/// A skill's parsed frontmatter.
#[derive(Default)]
struct SkillFront {
    name: Option<String>,
    description: Option<String>,
    /// A capability the runtime must have for the skill to exist at all (today:
    /// `subscription`). Absent → always available.
    requires: Option<String>,
    /// `always: true` — not a skill to reach for, a standing instruction. Its body
    /// rides in the preamble every turn instead of waiting for `skill_apply`.
    always: bool,
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

// ── rig tools ────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct SkillList(Arc<SkillStore>);

impl Tool for SkillList {
    const NAME: &'static str = "skill_list";
    type Error = Infallible;
    type Args = ListArgs;
    type Output = Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Browse the installed skills, one page at a time (name + when to use \
                          each). Pass `page` (1-indexed; default 1); the result carries `pages` \
                          and `total`. When there are many skills, prefer skill_search to jump \
                          straight to the relevant ones."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "page": { "type": "integer", "description": "1-indexed page (default 1)" }
                }
            }),
        }
    }

    async fn call(&self, args: ListArgs) -> Result<Value, Infallible> {
        Ok(self.0.list_json(args.page.unwrap_or(1)))
    }
}

#[derive(Clone)]
pub struct SkillSearch(Arc<SkillStore>);

impl Tool for SkillSearch {
    const NAME: &'static str = "skill_search";
    type Error = Infallible;
    type Args = SearchArgs;
    type Output = Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Find skills by need when you have many and the per-turn catalog is only \
                          a summary. Give `query` (a few keywords about the task); returns the \
                          best-matching skills (name + when-to-use), ranked by name/description \
                          match. Then skill_apply the one that fits. Optional `limit` (default 10)."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "keywords describing the task" },
                    "limit": { "type": "integer", "description": "max matches (default 10)" }
                },
                "required": ["query"]
            }),
        }
    }

    async fn call(&self, args: SearchArgs) -> Result<Value, Infallible> {
        let limit = args.limit.unwrap_or(SEARCH_LIMIT).clamp(1, SEARCH_MAX);
        Ok(self.0.search_json(&args.query, limit))
    }
}

#[derive(Clone)]
pub struct SkillApply(Arc<SkillStore>);

impl Tool for SkillApply {
    const NAME: &'static str = "skill_apply";
    type Error = Infallible;
    type Args = ApplyArgs;
    type Output = Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Apply a skill by name: loads and returns its full instructions. Follow \
                          them exactly and literally — a skill is an authoritative recipe to \
                          execute, not a suggestion to paraphrase; do not invent steps, \
                          parameters, or rules not written in it, and quote it verbatim if asked \
                          to show it. The instructions are returned now but not kept in context \
                          afterward — re-apply if you need them again. The result also \
                          lists the skill's bundled files (if any); read one with skill_file."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": { "name": { "type": "string" } },
                "required": ["name"]
            }),
        }
    }

    async fn call(&self, args: ApplyArgs) -> Result<Value, Infallible> {
        Ok(self.0.apply_json(&args.name))
    }
}

#[derive(Clone)]
pub struct SkillFile(Arc<SkillStore>);

impl Tool for SkillFile {
    const NAME: &'static str = "skill_file";
    type Error = Infallible;
    type Args = FileArgs;
    type Output = Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Read one of an applied skill's bundled files IN PLACE (a template / \
                          reference / example it ships), by the skill name and a path relative to \
                          the skill — skill_apply lists what a skill bundles. Read-only; do your \
                          actual work in the file workspace, not here."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "path": { "type": "string" }
                },
                "required": ["name", "path"]
            }),
        }
    }

    async fn call(&self, args: FileArgs) -> Result<Value, Infallible> {
        Ok(self.0.file_json(&args.name, &args.path))
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct ListArgs {
    #[serde(default)]
    pub page: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct SearchArgs {
    pub query: String,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct ApplyArgs {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct FileArgs {
    pub name: String,
    pub path: String,
}

#[cfg(test)]
mod tests {
    use std::fs::{create_dir_all, remove_dir_all, write};

    use super::{meta_of, scan, SkillStore};

    /// A throwaway `skills/` tree; `skills` is a list of `(folder, SKILL.md body)`.
    fn skills_dir(tag: &str, skills: &[(&str, &str)]) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("albert-skills-test-{tag}"));
        let _ = remove_dir_all(&root);
        for (name, body) in skills {
            create_dir_all(root.join(name)).unwrap();
            write(root.join(name).join("SKILL.md"), body).unwrap();
        }
        root
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
        assert!(catalog.contains("Lead with the next action."), "got:\n{catalog}");
        assert!(catalog.contains("STANDING INSTRUCTIONS"), "got:\n{catalog}");
        // Not offered: absent from the pick-one list, which still holds the others.
        assert!(!catalog.contains("- style:"), "an in-force skill must not be offered:\n{catalog}");
        assert!(catalog.contains("- brief: pick me when asked"), "got:\n{catalog}");

        let _ = remove_dir_all(&dir);
    }

    #[test]
    fn standing_instructions_survive_a_paginated_catalog() {
        // With page=1 the list collapses to a count — the standing body must NOT
        // collapse with it, or the contract would vanish as skills accumulate.
        let dir = skills_dir(
            "always-paged",
            &[
                ("style", "---\nname: style\nalways: true\n---\nAction first."),
                ("a", "---\nname: a\ndescription: x\n---\nb"),
                ("b", "---\nname: b\ndescription: y\n---\nb"),
            ],
        );
        let catalog = SkillStore::load(dir.clone(), 5, 1, &[]).catalog();
        assert!(catalog.contains("Action first."), "got:\n{catalog}");
        assert!(catalog.contains("2 installed"), "in-force skills aren't counted:\n{catalog}");
        let _ = remove_dir_all(&dir);
    }

    #[test]
    fn a_skill_is_hidden_until_its_capability_is_present() {
        let dir = skills_dir(
            "gating",
            &[
                ("transcribe", "---\nname: transcribe\nrequires: subscription\n---\nbody"),
                ("brief", "---\nname: brief\ndescription: always here\n---\nbody"),
            ],
        );
        let without: Vec<String> = scan(&dir, &[]).into_iter().map(|s| s.name).collect();
        assert_eq!(without, vec!["brief"], "no subscription -> transcribe is absent");
        let with: Vec<String> = scan(&dir, &["subscription"]).into_iter().map(|s| s.name).collect();
        assert_eq!(with, vec!["brief", "transcribe"], "subscription -> both, sorted");
        let _ = remove_dir_all(&dir);
    }
}
