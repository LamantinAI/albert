//! Explicit per-run roots; never mutate process-global OCTO_CODE_WORKSPACE.
use std::{
    convert::Infallible,
    fs::{symlink_metadata, File},
    io::{ErrorKind, Read},
    path::{Component, Path, PathBuf},
};

use octo_code::write_atomic;
use rig::{completion::ToolDefinition, tool::Tool};
use serde::Deserialize;
use serde_json::{json, Value};

pub struct WorkspaceRead(pub PathBuf);
pub struct WorkspaceWrite(pub PathBuf);
#[derive(Deserialize)]
pub struct ReadArgs {
    pub path: String,
}
#[derive(Deserialize)]
pub struct WriteArgs {
    pub path: String,
    pub content: String,
}
impl Tool for WorkspaceRead {
    const NAME: &'static str = "read";
    type Error = Infallible;
    type Args = ReadArgs;
    type Output = Value;
    async fn definition(&self, _: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.into(),
            description: "Read UTF-8 text in your private run workspace; paths are relative."
                .into(),
            parameters: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
        }
    }
    async fn call(&self, args: ReadArgs) -> Result<Value, Infallible> {
        if let Err(error) = check_path(&self.0, &args.path) {
            return Ok(json!({"error":error}));
        }
        let mut bytes = Vec::new();
        let read = File::open(self.0.join(&args.path))
            .and_then(|file| file.take(262145).read_to_end(&mut bytes));
        Ok(match read {
            Ok(_) => {
                json!({"content":String::from_utf8_lossy(&bytes[..bytes.len().min(262144)]),"truncated":bytes.len()>262144})
            }
            Err(error) => json!({"error":error.to_string()}),
        })
    }
}
impl Tool for WorkspaceWrite {
    const NAME: &'static str = "write";
    type Error = Infallible;
    type Args = WriteArgs;
    type Output = Value;
    async fn definition(&self, _: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.into(),
            description: "Write UTF-8 text in your private run workspace; paths are relative."
                .into(),
            parameters: json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"]}),
        }
    }
    async fn call(&self, args: WriteArgs) -> Result<Value, Infallible> {
        if let Err(error) = check_path(&self.0, &args.path) {
            return Ok(json!({"error":error}));
        }
        Ok(
            match write_atomic(&self.0, &args.path, args.content.as_bytes()) {
                Ok(_) => json!({"ok":true,"path":args.path}),
                Err(error) => json!({"error":error.to_string()}),
            },
        )
    }
}

/// octo-workspace also permits shared /tmp artifacts. Child native file tools
/// deliberately use only their run directory, including when it lives in /tmp.
/// This is path confinement, not isolation from a separately granted forkd.
fn check_path(root: &Path, relative: &str) -> Result<(), String> {
    if relative.is_empty() {
        return Err("Empty workspace path.".into());
    }
    let mut path = root.to_path_buf();
    for component in Path::new(relative).components() {
        let Component::Normal(part) = component else {
            return Err("Use a relative path without traversal.".into());
        };
        path.push(part);
        match symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err("Workspace paths cannot follow symlinks.".into())
            }
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(())
}
