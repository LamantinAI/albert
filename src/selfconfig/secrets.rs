//! Secret file updates and permissions, separate from editable agent config.

use std::{
    fs::{read_to_string, remove_file, rename, set_permissions, File, OpenOptions, Permissions},
    io::{ErrorKind, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
};

use super::SelfConfig;
use rand::random;

impl SelfConfig {
    /// Add or replace `NAME=value` in `.env`. Reads the file internally to update in
    /// place, but never surfaces existing values (no read tool exposes them). Written
    /// atomically at mode 0600.
    pub(super) fn upsert_secret(&self, name: &str, value: &str) -> Result<&'static str, String> {
        validate_env_name(name)?;
        if value.contains('\n') || value.contains('\r') || value.contains('\0') {
            return Err("value must not contain a newline".into());
        }
        let path = self.env_path.as_path();
        let existing = match read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
            Err(_) => return Err("The secret file could not be read.".into()),
        };
        let assignment = format!(
            "{name}=\"{}\"",
            value
                .replace('\\', "\\\\")
                .replace('\"', "\\\"")
                .replace('$', "\\$")
        );
        let mut lines: Vec<String> = Vec::new();
        let mut found = false;
        for line in existing.lines() {
            let trimmed = line.trim_start();
            let candidate = trimmed
                .strip_prefix("export ")
                .unwrap_or(trimmed)
                .trim_start();
            if candidate
                .split_once('=')
                .is_some_and(|(key, _)| key.trim() == name)
            {
                if !found {
                    lines.push(assignment.clone());
                }
                found = true;
            } else {
                lines.push(line.to_string());
            }
        }
        if !found {
            lines.push(assignment);
        }
        let mut body = lines.join("\n");
        body.push('\n');
        write_secret_file(path, &body)?;
        Ok(if found { "updated" } else { "added" })
    }

    /// The NAMES of secrets set in `.env` (values never returned).
    pub(super) fn secret_names(&self) -> Vec<String> {
        let text = read_to_string(self.env_path.as_path()).unwrap_or_default();
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
        out.sort();
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
    let tmp = dir.join(format!(".env.tmp.{:032x}", random::<u128>()));
    {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(|e| e.to_string())?;
        f.write_all(content.as_bytes()).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
    }
    let _ = set_permissions(&tmp, Permissions::from_mode(0o600));
    rename(&tmp, path).map_err(|e| {
        let _ = remove_file(&tmp);
        e.to_string()
    })?;
    File::open(dir)
        .and_then(|file| file.sync_all())
        .map_err(|e| e.to_string())
}
