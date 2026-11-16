use super::{SelfConfig, ALLOWED};
use rig::{completion::ToolDefinition, tool::Tool};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    convert::Infallible,
    fs::{read_dir, read_to_string},
    path::Path,
};
#[derive(Debug, Deserialize)]
pub struct PathArg {
    pub path: String,
}
#[derive(Debug, Deserialize)]
pub struct ListArg {
    #[serde(default)]
    pub path: Option<String>,
}
#[derive(Clone)]
pub struct ConfigRead(pub(super) SelfConfig);
impl Tool for ConfigRead {
    const NAME: &'static str = "config_read";
    type Error = Infallible;
    type Args = PathArg;
    type Output = Value;

    async fn definition(&self, _p: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.into(),
            description: "Read one of your own configuration files (path relative to your deploy \
                          dir): albert.toml, soul.md, system.md, config/**, skills/**. Use this to \
                          see your REAL current config before changing anything — don't rely on \
                          memory. Owner-only."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": { "path": { "type": "string", "description": "e.g. albert.toml, config/connectors/search/search.toml" } },
                "required": ["path"]
            }),
        }
    }

    async fn call(&self, args: PathArg) -> Result<Value, Self::Error> {
        Ok(
            match self
                .0
                .resolve(&args.path)
                .and_then(|p| read_to_string(&p).map_err(|e| e.to_string()))
            {
                Ok(content) => json!({ "path": args.path, "content": content }),
                Err(e) => json!({ "error": e }),
            },
        )
    }
}
#[derive(Clone)]
pub struct ConfigList(pub(super) SelfConfig);
impl Tool for ConfigList {
    const NAME: &'static str = "config_list";
    type Error = Infallible;
    type Args = ListArg;
    type Output = Value;

    async fn definition(&self, _p: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.into(),
            description: "List your own config layout to discover what actually exists — the real \
                          file names, not what you remember. Omit `path` for the top level, or pass \
                          a dir like `config/connectors`. Owner-only."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": { "path": { "type": "string", "description": "a dir under the deploy root; omit for the top level" } }
            }),
        }
    }

    async fn call(&self, args: ListArg) -> Result<Value, Self::Error> {
        let rel = args.path.unwrap_or_default();
        // The top level is not itself in the allow-list; surface exactly the allowed
        // entries there so listing reveals the real, editable layout.
        if rel.trim().is_empty() || rel == "." {
            let entries: Vec<Value> = ALLOWED
                .iter()
                .filter(|e| self.0.root.join(e).exists())
                .map(|e| json!({ "name": e, "kind": if self.0.root.join(e).is_dir() { "dir" } else { "file" } }))
                .collect();
            return Ok(json!({ "path": "", "entries": entries }));
        }
        Ok(match self.0.resolve(&rel) {
            Ok(dir) => match list_dir(&dir) {
                Ok(entries) => json!({ "path": rel, "entries": entries }),
                Err(e) => json!({ "error": e }),
            },
            Err(e) => json!({ "error": e }),
        })
    }
}
fn list_dir(dir: &Path) -> Result<Vec<Value>, String> {
    let mut out = Vec::new();
    for entry in read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let kind = if entry.path().is_dir() { "dir" } else { "file" };
        out.push(json!({ "name": name, "kind": kind }));
    }
    out.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    Ok(out)
}
