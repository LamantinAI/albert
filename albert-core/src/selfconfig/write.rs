use super::{harden_path, SelfConfig};
use octo_code::write_atomic;
use rig::{completion::ToolDefinition, tool::Tool};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{convert::Infallible, fs::read_to_string};
#[derive(Debug, Deserialize)]
pub struct WriteArg {
    pub path: String,
    pub content: String,
}
#[derive(Debug, Deserialize)]
pub struct EditArg {
    pub path: String,
    /// Exact text to replace — must occur exactly once in the file.
    pub old: String,
    pub new: String,
}
#[derive(Clone)]
pub struct ConfigWrite(pub(super) SelfConfig);
impl Tool for ConfigWrite {
    const NAME: &'static str = "config_write";
    type Error = Infallible;
    type Args = WriteArg;
    type Output = Value;

    async fn definition(&self, _p: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.into(),
            description: "Write (create or overwrite) one of your own config files — for a whole \
                          new file, e.g. a new connector manifest under config/connectors/<id>/. \
                          For a small change to an existing file prefer config_edit. Tell the owner \
                          what you're changing first; apply it afterwards with the restart tool. \
                          Owner-only."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string", "description": "the full new file contents" }
                },
                "required": ["path", "content"]
            }),
        }
    }

    async fn call(&self, args: WriteArg) -> Result<Value, Self::Error> {
        // resolve() enforces the allow-list; write_atomic re-jails to the root and writes.
        Ok(match self.0.resolve(&args.path) {
            Ok(_) => match write_atomic(&self.0.root, &args.path, args.content.as_bytes()) {
                Ok(written) => {
                    harden_path(&self.0.root, &written);
                    json!({ "ok": true, "path": args.path, "bytes": args.content.len() })
                }
                Err(e) => json!({ "ok": false, "error": e.to_string() }),
            },
            Err(e) => json!({ "ok": false, "error": e }),
        })
    }
}
#[derive(Clone)]
pub struct ConfigEdit(pub(super) SelfConfig);
impl Tool for ConfigEdit {
    const NAME: &'static str = "config_edit";
    type Error = Infallible;
    type Args = EditArg;
    type Output = Value;

    async fn definition(&self, _p: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.into(),
            description: "Change part of an existing config file: replace `old` with `new`. `old` \
                          must appear EXACTLY ONCE (include enough surrounding text to be unique), \
                          so the edit is unambiguous. Read the file first. Apply the change with the \
                          restart tool afterwards. Owner-only."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "old":  { "type": "string", "description": "exact text to replace; must occur once" },
                    "new":  { "type": "string" }
                },
                "required": ["path", "old", "new"]
            }),
        }
    }

    async fn call(&self, args: EditArg) -> Result<Value, Self::Error> {
        Ok(match self.edit(&args) {
            Ok(v) => v,
            Err(e) => json!({ "ok": false, "error": e }),
        })
    }
}
impl ConfigEdit {
    pub(super) fn edit(&self, args: &EditArg) -> Result<Value, String> {
        let path = self.0.resolve(&args.path)?;
        let text = read_to_string(&path).map_err(|e| e.to_string())?;
        let count = text.matches(&args.old).count();
        match count {
            0 => Err("`old` text not found in the file".into()),
            1 => {
                let updated = text.replacen(&args.old, &args.new, 1);
                let _ = path; // validated above; write_atomic re-jails from the root
                let written = write_atomic(&self.0.root, &args.path, updated.as_bytes())
                    .map_err(|e| e.to_string())?;
                harden_path(&self.0.root, &written);
                Ok(json!({ "ok": true, "path": args.path }))
            }
            n => Err(format!(
                "`old` occurs {n} times — add surrounding text so it is unique"
            )),
        }
    }
}
