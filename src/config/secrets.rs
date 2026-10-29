use std::{
    env::var,
    path::{Path, PathBuf},
};

use dotenvy::from_path_iter;
use kaeru_core::KaeruConfig;

use super::Config;

/// Read the owner's saved secret without mutating a multithreaded process's
/// environment. A deliberately empty file value overrides an older environment.
pub(super) fn read_secret(directory: &Path, name: &str) -> Option<String> {
    if let Ok(entries) = from_path_iter(directory.join(".env")) {
        for (key, value) in entries.flatten() {
            if key == name {
                return Some(value);
            }
        }
    }
    var(name).ok()
}

impl Config {
    pub(crate) fn embedded_vault_path(&self) -> PathBuf {
        if let Some(value) = self.secret("KAERU_VAULT_PATH") {
            return PathBuf::from(value);
        }
        let previous = KaeruConfig::default().vault_path;
        if previous.exists() {
            previous
        } else {
            self.deploy_dir.join("state/kaeru")
        }
    }

    pub(crate) fn model_key(&self, reference: Option<&str>) -> Option<String> {
        let name = reference.unwrap_or(&self.openai_key_env);
        match read_secret(&self.deploy_dir, name) {
            Some(value) => (!value.trim().is_empty()).then_some(value),
            None if reference.is_none() => self.api_key.clone(),
            None => None,
        }
    }

    pub(crate) fn secret(&self, name: &str) -> Option<String> {
        read_secret(&self.deploy_dir, name).filter(|value| !value.trim().is_empty())
    }
}
