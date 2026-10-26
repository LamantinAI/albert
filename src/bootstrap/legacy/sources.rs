use std::{collections::BTreeSet, fs::read_to_string, path::Path};

use serde::Deserialize;
use toml::{from_str, Value};

use crate::{
    error::{Error, Result},
    manifests::connector_files,
};

#[derive(Deserialize)]
struct Manifest {
    connector: Header,
}
#[derive(Deserialize)]
struct Header {
    id: String,
    #[serde(rename = "type")]
    kind: String,
}

/// Host configuration chooses a deterministic custodian for old bare keys. This
/// does not read, seed or modify the Telegram ACL and grants no runtime access.
pub(super) fn telegram_ids(manifest: &Path, enabled: bool) -> Result<Vec<String>> {
    if !enabled && !manifest.exists() {
        return Ok(Vec::new());
    }
    let _: Value = from_str(&read_to_string(manifest)?)?;
    let mut ids = BTreeSet::new();
    let mut telegram = Vec::new();
    for path in connector_files(manifest) {
        if !path.exists() {
            continue;
        }
        let file: Manifest = from_str(&read_to_string(path)?)?;
        if !ids.insert(file.connector.id.clone()) {
            return Err(Error::Config(
                "Duplicate connector IDs prevent automatic history migration.".into(),
            ));
        }
        if file.connector.kind == "telegram" {
            telegram.push(file.connector.id);
        }
    }
    telegram.sort();
    if let Some(index) = telegram.iter().position(|id| id == "telegram") {
        telegram.swap(0, index);
    }
    Ok(telegram)
}
