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

mod read;
mod write;
pub use self::{
    read::{ConfigList, ConfigRead, ListArg, PathArg},
    write::{ConfigEdit, ConfigWrite, EditArg, WriteArg},
};

use std::fs::{set_permissions, Permissions};
mod secrets;

use std::{convert::Infallible, path::Path, path::PathBuf, sync::Arc};

use octo_code::resolve_in_root;
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
        Self {
            root: Arc::new(root),
            env_path: Arc::new(env_path),
        }
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

    pub fn panel_upsert_secret(&self, name: &str, value: &str) -> Result<bool, String> {
        self.upsert_secret(name, value)
            .map(|state| state == "updated")
    }

    pub fn panel_set_secret(&self, name: &str, value: &str) -> Result<(), String> {
        self.upsert_secret(name, value).map(|_| ())
    }

    pub fn panel_secret_names(&self) -> Vec<String> {
        self.secret_names()
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
    let _ = set_permissions(file, Permissions::from_mode(0o644));
    let mut cur = file.parent();
    while let Some(dir) = cur {
        if dir == root {
            break;
        }
        let _ = set_permissions(dir, Permissions::from_mode(0o755));
        cur = dir.parent();
    }
}
#[cfg(not(unix))]
fn harden_path(_root: &Path, _file: &Path) {}

// ── config_read ───────────────────────────────────────────────────────────────

// ── config_list ───────────────────────────────────────────────────────────────

// ── config_write ──────────────────────────────────────────────────────────────

// ── config_edit ───────────────────────────────────────────────────────────────

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
    type Error = Infallible;
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
    type Error = Infallible;
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

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

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
        assert!(
            sc.resolve("state/history.db").is_err(),
            "state must be denied"
        );
        assert!(
            sc.resolve("../outside").is_err(),
            "jail escape must be denied"
        );
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
        assert!(env.contains("JIRA_TOKEN=\"abc123\""));
        assert!(env.contains("EXISTING=\"new\"") && !env.contains("EXISTING=old"));
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
        let mode = fs::metadata(dir.path().join(".env"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
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
        assert!(edit
            .edit(&EditArg {
                path: "albert.toml".into(),
                old: "= 1".into(),
                new: "= 2".into()
            })
            .is_err());
        // Unique → applied.
        assert!(edit
            .edit(&EditArg {
                path: "albert.toml".into(),
                old: "a = 1".into(),
                new: "a = 2".into()
            })
            .is_ok());
        assert_eq!(
            fs::read_to_string(root.join("albert.toml")).unwrap(),
            "a = 2\nb = 1\n"
        );
    }
    #[test]
    fn saved_secrets_round_trip_literals_and_replace_exported_or_spaced_assignments() {
        use std::{fs::read_to_string, fs::write};

        use dotenvy::from_path_iter;
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let config = super::SelfConfig::new(dir.path().to_path_buf());
        let path = dir.path().join(".env");
        write(
            &path,
            "export ROUNDTRIP_SECRET = old\nROUNDTRIP_SECRET=duplicate\nOTHER=retained\n",
        )
        .unwrap();
        for value in [
            "abc$UNSET",
            "a b # c",
            "'quoted' and \"double\"",
            r"C:\tmp\secret",
            "",
            "日本語 café",
        ] {
            config.upsert_secret("ROUNDTRIP_SECRET", value).unwrap();
            let entries: Vec<_> = from_path_iter(&path)
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            assert_eq!(
                entries
                    .iter()
                    .filter(|(key, _)| key == "ROUNDTRIP_SECRET")
                    .count(),
                1
            );
            assert_eq!(
                entries
                    .iter()
                    .find(|(key, _)| key == "ROUNDTRIP_SECRET")
                    .unwrap()
                    .1,
                value
            );
            assert_eq!(
                entries.iter().find(|(key, _)| key == "OTHER").unwrap().1,
                "retained"
            );
        }
        assert!(read_to_string(path).unwrap().contains("OTHER=retained"));
    }
}
