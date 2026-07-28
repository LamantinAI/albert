//! Owner-only **self-configuration** — file tools that let Albert read and edit its
//! own configuration, jailed to the deployment directory and allow-listed.
//!
//! This is deliberately separate from the octo-code workspace tools (`read`/`write`/…
//! jailed to the throwaway `$OCTO_CODE_WORKSPACE`): those are the agent's scratch
//! space; these reach the *real* deploy files (`albert.toml`, `soul.md`, `system.md`,
//! connector manifests under `config/`, `skills/`). Two guards keep it safe:
//!
//! 1. **Owner-only.** Like the restart tool, these are added to the toolset only on an
//!    owner turn — a non-owner never sees them.
//! 2. **Jail + allow-list.** Every path is resolved inside the deploy dir (no `..`
//!    escape) AND must fall under [`ALLOWED`]; the binary, `.env` (secrets!), `state/`
//!    and the kaeru vault are off-limits. Combined with the deploy's file ownership
//!    (config is owner-writable by `albert` but only group-readable, so forkd's
//!    dropped `albert-scripts` can read but not write), the agent can edit its config
//!    while sandboxed scripts cannot — the isolation hole closed the right way.
//!
//! After editing, Albert applies the change with the `restart` tool (a connector to
//! reload its manifest, or `process` for `albert.toml`). The `self-config` skill is
//! the authoritative map + procedure.

use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use octo_code::{resolve_in_root, write_atomic};
use rig::{completion::ToolDefinition, tool::Tool};
use serde::Deserialize;
use serde_json::{json, Value};

/// Top-level files/dirs the agent may touch, relative to the deploy root. Everything
/// else — the `albert` binary, `.env`, `state/`, `kaeru/` — is denied.
const ALLOWED: &[&str] = &["albert.toml", "soul.md", "system.md", "config", "skills"];

/// Owner-only self-config faculty, jailed to the deploy dir. Cheap to clone; build one
/// per owner turn and hand out its tools.
#[derive(Clone)]
pub struct SelfConfig {
    root: Arc<PathBuf>,
    /// The secrets file (`<deploy>/.env`). Handled only by the dedicated secret tools
    /// below — the general file tools' allow-list refuses it. Set/upsert only; values
    /// are never read back out to the model.
    env_path: Arc<PathBuf>,
}

impl SelfConfig {
    pub fn new(root: PathBuf) -> Self {
        let env_path = root.join(".env");
        Self { root: Arc::new(root), env_path: Arc::new(env_path) }
    }

    pub fn read_tool(&self) -> ConfigRead {
        ConfigRead(self.clone())
    }
    pub fn list_tool(&self) -> ConfigList {
        ConfigList(self.clone())
    }
    pub fn write_tool(&self) -> ConfigWrite {
        ConfigWrite(self.clone())
    }
    pub fn edit_tool(&self) -> ConfigEdit {
        ConfigEdit(self.clone())
    }
    pub fn set_secret_tool(&self) -> SetSecret {
        SetSecret(self.clone())
    }
    pub fn list_secrets_tool(&self) -> ListSecrets {
        ListSecrets(self.clone())
    }

    /// Resolve a deploy-relative path inside the jail AND the allow-list.
    fn resolve(&self, rel: &str) -> Result<PathBuf, String> {
        let abs = resolve_in_root(&self.root, rel).map_err(|e| e.to_string())?;
        if !self.allowed(&abs) {
            return Err(format!(
                "`{rel}` is not editable. Allowed: albert.toml, soul.md, system.md, \
                 config/**, skills/** — never .env, the binary, state/ or kaeru/."
            ));
        }
        Ok(abs)
    }

    fn allowed(&self, abs: &Path) -> bool {
        ALLOWED.iter().any(|entry| {
            let allowed = self.root.join(entry);
            abs == allowed || abs.starts_with(&allowed)
        })
    }
}

/// After writing a config file, tighten its perms so a sandboxed forkd script (which
/// runs as `albert-scripts`, in the agent's group) cannot tamper with it — the service
/// runs `UMask=0007`, which would otherwise leave config group-writable. The file
/// becomes owner-write / group-read (`0644`); its ancestor dirs up to (not including)
/// the root become owner-write only (`0755`), so scripts can't create/delete/rename in
/// them either (directory write, not file mode, governs that).
#[cfg(unix)]
fn harden_path(root: &Path, file: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(file, fs::Permissions::from_mode(0o644));
    let mut cur = file.parent();
    while let Some(dir) = cur {
        if dir == root {
            break;
        }
        let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o755));
        cur = dir.parent();
    }
}
#[cfg(not(unix))]
fn harden_path(_root: &Path, _file: &Path) {}

#[derive(Debug, Deserialize)]
pub struct PathArg {
    pub path: String,
}

#[derive(Debug, Deserialize)]
pub struct ListArg {
    #[serde(default)]
    pub path: Option<String>,
}

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

// ── config_read ───────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct ConfigRead(SelfConfig);

impl Tool for ConfigRead {
    const NAME: &'static str = "config_read";
    type Error = std::convert::Infallible;
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
        Ok(match self.0.resolve(&args.path).and_then(|p| fs::read_to_string(&p).map_err(|e| e.to_string())) {
            Ok(content) => json!({ "path": args.path, "content": content }),
            Err(e) => json!({ "error": e }),
        })
    }
}

// ── config_list ───────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct ConfigList(SelfConfig);

impl Tool for ConfigList {
    const NAME: &'static str = "config_list";
    type Error = std::convert::Infallible;
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
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let kind = if entry.path().is_dir() { "dir" } else { "file" };
        out.push(json!({ "name": name, "kind": kind }));
    }
    out.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    Ok(out)
}

// ── config_write ──────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct ConfigWrite(SelfConfig);

impl Tool for ConfigWrite {
    const NAME: &'static str = "config_write";
    type Error = std::convert::Infallible;
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

// ── config_edit ───────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct ConfigEdit(SelfConfig);

impl Tool for ConfigEdit {
    const NAME: &'static str = "config_edit";
    type Error = std::convert::Infallible;
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
    fn edit(&self, args: &EditArg) -> Result<Value, String> {
        let path = self.0.resolve(&args.path)?;
        let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
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
            n => Err(format!("`old` occurs {n} times — add surrounding text so it is unique")),
        }
    }
}

// ── config_set_secret ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct SetSecretArg {
    /// The env-var name, e.g. `JIRA_TOKEN`.
    pub name: String,
    /// The secret value. Never read back out; redacted from the status feed and the
    /// action log so it doesn't land in chat or history.
    pub value: String,
}

#[derive(Clone)]
pub struct SetSecret(SelfConfig);

impl Tool for SetSecret {
    const NAME: &'static str = "config_set_secret";
    type Error = std::convert::Infallible;
    type Args = SetSecretArg;
    type Output = Value;

    async fn definition(&self, _p: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.into(),
            description: "Set (add or replace) one secret in your .env — the value a connector \
                          manifest references by env-var, e.g. the JIRA_TOKEN behind \
                          ${secret.jira_token}. Write-only: you can set a secret but never read \
                          existing ones back. After setting it, apply with restart. NEVER repeat the \
                          value in your reply. Owner-only."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name":  { "type": "string", "description": "env var name, e.g. JIRA_TOKEN" },
                    "value": { "type": "string", "description": "the secret value" }
                },
                "required": ["name", "value"]
            }),
        }
    }

    async fn call(&self, args: SetSecretArg) -> Result<Value, Self::Error> {
        Ok(match self.0.upsert_secret(&args.name, &args.value) {
            Ok(action) => json!({ "ok": true, "name": args.name, "action": action }),
            Err(e) => json!({ "ok": false, "error": e }),
        })
    }
}

// ── config_list_secrets ───────────────────────────────────────────────────────

#[derive(Debug, Default, Deserialize)]
pub struct NoArg {}

#[derive(Clone)]
pub struct ListSecrets(SelfConfig);

impl Tool for ListSecrets {
    const NAME: &'static str = "config_list_secrets";
    type Error = std::convert::Infallible;
    type Args = NoArg;
    type Output = Value;

    async fn definition(&self, _p: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.into(),
            description: "List the NAMES of the secrets currently set in your .env (never the \
                          values) — so you can check whether e.g. JIRA_TOKEN is set before telling \
                          the owner what's missing. Owner-only."
                .into(),
            parameters: json!({ "type": "object", "properties": {} }),
        }
    }

    async fn call(&self, _args: NoArg) -> Result<Value, Self::Error> {
        Ok(json!({ "names": self.0.secret_names() }))
    }
}

impl SelfConfig {
    /// Add or replace `NAME=value` in `.env`. Reads the file internally to update in
    /// place, but never surfaces existing values (no read tool exposes them). Written
    /// atomically at mode 0600.
    fn upsert_secret(&self, name: &str, value: &str) -> Result<&'static str, String> {
        validate_env_name(name)?;
        if value.contains('\n') || value.contains('\r') {
            return Err("value must not contain a newline".into());
        }
        let path = self.env_path.as_path();
        let existing = fs::read_to_string(path).unwrap_or_default();
        let prefix = format!("{name}=");
        let mut lines: Vec<String> = Vec::new();
        let mut found = false;
        for line in existing.lines() {
            let trimmed = line.trim_start();
            if !trimmed.starts_with('#') && trimmed.starts_with(&prefix) {
                lines.push(format!("{name}={value}"));
                found = true;
            } else {
                lines.push(line.to_string());
            }
        }
        if !found {
            lines.push(format!("{name}={value}"));
        }
        let mut body = lines.join("\n");
        body.push('\n');
        write_secret_file(path, &body)?;
        Ok(if found { "updated" } else { "added" })
    }

    /// The NAMES of secrets set in `.env` (values never returned).
    fn secret_names(&self) -> Vec<String> {
        let text = fs::read_to_string(self.env_path.as_path()).unwrap_or_default();
        let mut out: Vec<String> = text
            .lines()
            .filter_map(|l| {
                let l = l.trim();
                if l.starts_with('#') {
                    return None;
                }
                let name = l.split_once('=')?.0.trim().to_string();
                (!name.is_empty()).then_some(name)
            })
            .collect();
        out.dedup();
        out
    }
}

/// A valid env-var name: letters/digits/underscore, not starting with a digit.
fn validate_env_name(name: &str) -> Result<(), String> {
    let ok = !name.is_empty()
        && name.chars().enumerate().all(|(i, c)| {
            if i == 0 {
                c.is_ascii_alphabetic() || c == '_'
            } else {
                c.is_ascii_alphanumeric() || c == '_'
            }
        });
    ok.then_some(())
        .ok_or_else(|| "secret name must be letters/digits/underscore, not starting with a digit (e.g. JIRA_TOKEN)".into())
}

/// Atomic write of `.env` at mode 0600 (owner-only; systemd reads it as root, the agent
/// never reads it back, forkd's albert-scripts cannot touch it).
fn write_secret_file(path: &Path, content: &str) -> Result<(), String> {
    let dir = path.parent().ok_or("bad .env path")?;
    let tmp = dir.join(format!(".env.tmp.{}", std::process::id()));
    {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(|e| e.to_string())?;
        f.write_all(content.as_bytes()).map_err(|e| e.to_string())?;
        f.flush().map_err(|e| e.to_string())?;
    }
    let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600));
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        e.to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn allows_config_denies_secrets_and_escape() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fs::write(root.join("albert.toml"), "m=1").unwrap();
        fs::write(root.join(".env"), "S=1").unwrap();
        let sc = SelfConfig::new(root.clone());

        assert!(sc.resolve("albert.toml").is_ok());
        assert!(sc.resolve("config/connectors/x/x.toml").is_ok());
        assert!(sc.resolve(".env").is_err(), "secrets must be denied");
        assert!(sc.resolve("albert").is_err(), "binary must be denied");
        assert!(sc.resolve("state/history.db").is_err(), "state must be denied");
        assert!(sc.resolve("../outside").is_err(), "jail escape must be denied");
    }

    #[test]
    fn set_secret_upserts_and_lists_names_only() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fs::write(root.join(".env"), "# comment\nEXISTING=old\n").unwrap();
        let sc = SelfConfig::new(root.clone());

        assert_eq!(sc.upsert_secret("JIRA_TOKEN", "abc123").unwrap(), "added");
        assert_eq!(sc.upsert_secret("EXISTING", "new").unwrap(), "updated");
        let env = fs::read_to_string(root.join(".env")).unwrap();
        assert!(env.contains("JIRA_TOKEN=abc123"));
        assert!(env.contains("EXISTING=new") && !env.contains("EXISTING=old"));
        assert!(env.contains("# comment"), "comments preserved");

        // Listing returns names, never values.
        let names = sc.secret_names();
        assert!(names.contains(&"JIRA_TOKEN".to_string()));
        assert!(names.contains(&"EXISTING".to_string()));

        // Bad names refused.
        assert!(sc.upsert_secret("2BAD", "x").is_err());
        assert!(sc.upsert_secret("has space", "x").is_err());
        // Newline in value refused.
        assert!(sc.upsert_secret("OK_NAME", "a\nb").is_err());
    }

    #[test]
    fn secret_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let sc = SelfConfig::new(dir.path().to_path_buf());
        sc.upsert_secret("K", "v").unwrap();
        let mode = fs::metadata(dir.path().join(".env")).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "secrets file must be 0600");
    }

    #[test]
    fn edit_requires_a_unique_match() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fs::write(root.join("albert.toml"), "a = 1\nb = 1\n").unwrap();
        let sc = SelfConfig::new(root.clone());
        let edit = ConfigEdit(sc);

        // "= 1" occurs twice → refused.
        assert!(edit.edit(&EditArg { path: "albert.toml".into(), old: "= 1".into(), new: "= 2".into() }).is_err());
        // Unique → applied.
        assert!(edit.edit(&EditArg { path: "albert.toml".into(), old: "a = 1".into(), new: "a = 2".into() }).is_ok());
        assert_eq!(fs::read_to_string(root.join("albert.toml")).unwrap(), "a = 2\nb = 1\n");
    }
}
