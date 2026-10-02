//! Secret file updates and permissions, separate from editable agent config.

use std::{
    fs,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
};

use super::SelfConfig;

impl SelfConfig {
    /// Add or replace `NAME=value` in `.env`. Reads the file internally to update in
    /// place, but never surfaces existing values (no read tool exposes them). Written
    /// atomically at mode 0600.
    pub(super) fn upsert_secret(&self, name: &str, value: &str) -> Result<&'static str, String> {
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
    pub(super) fn secret_names(&self) -> Vec<String> {
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
