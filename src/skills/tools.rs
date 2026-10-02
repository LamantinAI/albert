//! Rig adapters for skill discovery and progressive instruction loading.

use std::{convert::Infallible, sync::Arc};

use rig::{completion::ToolDefinition, tool::Tool};
use serde::Deserialize;
use serde_json::{json, Value};

use super::{SkillStore, SEARCH_LIMIT, SEARCH_MAX};

// ── rig tools ────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct SkillList(pub(super) Arc<SkillStore>);

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
pub struct SkillSearch(pub(super) Arc<SkillStore>);

impl Tool for SkillSearch {
    const NAME: &'static str = "skill_search";
    type Error = Infallible;
    type Args = SearchArgs;
    type Output = Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description:
                "Find skills by need when you have many and the per-turn catalog is only \
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
pub struct SkillApply(pub(super) Arc<SkillStore>);

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
pub struct SkillFile(pub(super) Arc<SkillStore>);

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
