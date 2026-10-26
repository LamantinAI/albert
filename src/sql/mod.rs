//! SQL assembly wiring; internal storage remains owned by its existing services.
use std::{fs::read_to_string, path::PathBuf, sync::Arc};

use kaeru_core::KaeruConfig;
use octo_approval::Authority;
use octo_connector_sql::factory;
use octo_core::{Connector, ConnectorId, FactoryContext};
use toml::Value;

use crate::{
    config::{history::sqlite_path, Config},
    error::{Error, Result},
    manifests::connector_files,
    memory::config::MemoryConfig,
};

pub fn protected_paths(config: &Config) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    paths.push(sqlite_path(config.history.as_deref(), &config.deploy_dir)?);
    paths.push(config.history_backup_dir.clone());
    if matches!(&config.memory, MemoryConfig::Embedded) {
        let vault =
            KaeruConfig::from_env().map_err(|e| Error::Config(format!("memory path: {e}")))?;
        paths.push(vault.vault_path);
    }
    Ok(paths)
}

/// Console mode still loads explicitly configured SQL organs, without starting
/// Telegram/calendar connectors whose credentials may be absent locally.
pub fn console_sql_connectors(
    config: &Config,
    authority: Authority,
) -> Result<Vec<Arc<dyn Connector>>> {
    let factory = factory(authority, protected_paths(config)?);
    let mut connectors = Vec::new();
    for path in connector_files(&config.connectors_manifest) {
        let Ok(text) = read_to_string(&path) else {
            continue;
        };
        let value: Value = text.parse()?;
        if value
            .get("connector")
            .and_then(|v| v.get("type"))
            .and_then(Value::as_str)
            != Some("sql")
        {
            continue;
        }
        let id = value
            .get("connector")
            .and_then(|v| v.get("id"))
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Config("SQL connector needs an id".into()))?;
        let parent = path
            .parent()
            .ok_or_else(|| Error::Config("SQL manifest needs a parent".into()))?;
        connectors.push(
            factory
                .create(
                    ConnectorId::new(id),
                    &value,
                    FactoryContext { base_dir: parent },
                )
                .map_err(|e| Error::Config(format!("SQL connector: {e}")))?,
        );
    }
    Ok(connectors)
}
