//! Base configuration required before optional cognition can be activated.
use std::{fs::read_to_string, path::Path};

use toml::{from_str, to_string, Table};

use super::Config;
use crate::error::{Error, Result};

impl Config {
    pub fn parse_control_document(document: &Table, path: &Path) -> Result<Self> {
        let mut core = Table::new();
        for key in [
            "history",
            "sessions",
            "authorization",
            "scheduler",
            "prompt",
            "skills",
            "code",
            "timezone",
            "subscription",
            "connectors_manifest",
        ] {
            if let Some(value) = document.get(key) {
                core.insert(key.into(), value.clone());
            }
        }
        Self::parse_text(
            &to_string(&core).map_err(|_| Error::Config("Invalid control settings.".into()))?,
            path,
        )
    }
    pub fn load_control_from(path: &Path) -> Result<Self> {
        let text = if path.exists() {
            read_to_string(path)?
        } else {
            String::new()
        };
        let document: Table = from_str(&text)?;
        Self::parse_control_document(&document, path)
    }
}
