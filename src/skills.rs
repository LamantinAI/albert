//! Declarative skills — a `skills/` folder of `SKILL.md` files (instructions, no
//! scripts yet). At startup we scan the **catalog** (name + when-to-use, from each
//! file's frontmatter). Only the catalog is kept in front of the agent each turn —
//! the full body would clutter context. `skill_apply` returns a skill's instructions
//! on demand and caches the body in a small **LRU RAM read-through cache** (default
//! 5), so re-applying a recent skill is a fast RAM hit, not a disk read. The agent
//! lists (`skill_list`) and applies (`skill_apply`).
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

/// One skill's catalog entry (parsed frontmatter) + its folder.
struct SkillMeta {
    name: String,
    when: String,
    /// The skill's own folder (`skills/<name>/`) — holds `SKILL.md` and any bundled
    /// resources. `skill_file` reads are jailed to it.
    dir: PathBuf,
}

struct Inner {
    catalog: Vec<SkillMeta>,
    /// LRU read-through cache of loaded `(name, body)`; front = most-recently applied.
    cache: VecDeque<(String, String)>,
    cache_cap: usize,
}

/// Catalog of installed skills + an LRU cache of applied skill bodies.
pub struct SkillStore {
    inner: Mutex<Inner>,
}

impl SkillStore {
    /// Scan `dir` for `*/SKILL.md` and build the catalog. A missing `dir` yields an
    /// empty catalog (skills are optional).
    pub fn load(dir: PathBuf, cache_cap: usize) -> Arc<Self> {
        let catalog = scan(&dir);
        info!(skills = catalog.len(), dir = %dir.display(), "skills: catalog loaded");
        Arc::new(Self {
            inner: Mutex::new(Inner {
                catalog,
                cache: VecDeque::new(),
                cache_cap: cache_cap.max(1),
            }),
        })
    }

    /// The always-visible menu for the preamble.
    pub fn catalog(&self) -> String {
        let inner = self.inner.lock().unwrap();
        if inner.catalog.is_empty() {
            return "Skills: (none installed).".to_string();
        }
        let lines: Vec<String> = inner
            .catalog
            .iter()
            .map(|s| format!("- {}: {}", s.name, s.when))
            .collect();
        format!(
            "Skills available (apply one with skill_apply when it fits the task; skill_list to \
             re-list):\n{}",
            lines.join("\n")
        )
    }

    fn list_json(&self) -> Value {
        let inner = self.inner.lock().unwrap();
        let items: Vec<Value> = inner
            .catalog
            .iter()
            .map(|s| json!({ "name": s.name, "when": s.when }))
            .collect();
        json!({ "skills": items })
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
    pub fn apply_tool(self: &Arc<Self>) -> SkillApply {
        SkillApply(Arc::clone(self))
    }
    pub fn file_tool(self: &Arc<Self>) -> SkillFile {
        SkillFile(Arc::clone(self))
    }
}

/// Scan `skills/<name>/SKILL.md` into catalog entries, sorted by name.
fn scan(dir: &Path) -> Vec<SkillMeta> {
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
        let (name, when) = meta_of(&text);
        let name = name.unwrap_or_else(|| p.file_name().unwrap_or_default().to_string_lossy().into_owned());
        out.push(SkillMeta {
            name,
            when: when.unwrap_or_else(|| "(no description)".to_string()),
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
fn meta_of(text: &str) -> (Option<String>, Option<String>) {
    let (fm, _) = split_frontmatter(text);
    let mut name = None;
    let mut when = None;
    for line in fm.lines() {
        if let Some(v) = line.strip_prefix("name:") {
            name = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("description:") {
            when = Some(v.trim().to_string());
        }
    }
    (name, when)
}

// ── rig tools ────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct SkillList(Arc<SkillStore>);

impl Tool for SkillList {
    const NAME: &'static str = "skill_list";
    type Error = Infallible;
    type Args = NoArgs;
    type Output = Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "List the installed skills (name + when to use each). The catalog is also \
                          shown to you each turn."
                .to_string(),
            parameters: json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(&self, _args: NoArgs) -> Result<Value, Infallible> {
        Ok(self.0.list_json())
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
pub struct NoArgs {}

#[derive(Debug, Deserialize)]
pub struct ApplyArgs {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct FileArgs {
    pub name: String,
    pub path: String,
}
