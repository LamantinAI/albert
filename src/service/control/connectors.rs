//! A new connector is a normal manifest in the configured connector directory.
use std::{path::PathBuf, str::from_utf8};

use toml::{from_str, to_string_pretty, Table, Value};

use super::ControlService;
use crate::{
    api::errors::{ApiError, ApiResult},
    service::panel_models::files::{atomic_write, checked, read_optional, root},
};

impl ControlService {
    pub(super) fn new_connector_path(&self, id: &str) -> ApiResult<PathBuf> {
        let name = id
            .strip_prefix("connector:")
            .ok_or_else(ApiError::not_found)?;
        validate_id(name)?;
        let base = &self.runtime.base;
        let directory = root(&base.deploy_dir)?;
        let topology = read_optional(&directory, &base.connectors_manifest, 1024 * 1024)?;
        let document: Table = match topology {
            Some(bytes) => from_str(
                from_utf8(&bytes)
                    .map_err(|_| ApiError::invalid("Invalid connector topology encoding."))?,
            )
            .map_err(|_| {
                ApiError::invalid("Repair the connector topology before adding a connector.")
            })?,
            None => Table::new(),
        };
        let subdir = document
            .get("connectors")
            .and_then(|v| v.get("dir"))
            .and_then(|v| v.as_str())
            .unwrap_or("connectors");
        let parent = base
            .connectors_manifest
            .parent()
            .ok_or_else(ApiError::internal)?;
        checked(
            &directory,
            &parent.join(subdir).join(name).join(format!("{name}.toml")),
        )
    }

    pub(super) fn validate_connector(&self, id: &str, text: &str) -> Result<(), String> {
        let name = id
            .strip_prefix("connector:")
            .ok_or("Invalid connector document.")?;
        validate_id(name).map_err(|error| error.message)?;
        let value: Table = from_str(text).map_err(|_| "Invalid connector TOML.")?;
        let header = value
            .get("connector")
            .ok_or("A [connector] table is required.")?;
        if header.get("id").and_then(|v| v.as_str()) != Some(name) {
            return Err("Connector id must match the selected document.".into());
        }
        let kind = header
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or("A connector type is required.")?;
        if ![
            "telegram",
            "sql",
            "caldav",
            "storage",
            "forkd",
            "search",
            "browser",
            "http",
            "mail",
            "transcribe",
            "speak",
            "imagegen",
        ]
        .contains(&kind)
        {
            return Err("This connector type is not available in this Albert binary.".into());
        }
        Ok(())
    }

    pub(super) fn ensure_topology(&self) -> ApiResult<()> {
        let base = &self.runtime.base;
        let directory = root(&base.deploy_dir)?;
        let original = read_optional(&directory, &base.connectors_manifest, 1024 * 1024)?;
        let mut doc: Table = match &original {
            Some(bytes) => from_str(
                from_utf8(bytes)
                    .map_err(|_| ApiError::invalid("Invalid connector topology encoding."))?,
            )
            .map_err(|_| ApiError::invalid("Invalid connector topology."))?,
            None => Table::new(),
        };
        let section = doc
            .entry("connectors")
            .or_insert_with(|| Value::Table(Table::new()))
            .as_table_mut()
            .ok_or_else(|| ApiError::invalid("A [connectors] table is required."))?;
        if !section.contains_key("dir") {
            section.insert("dir".into(), Value::String("connectors".into()));
            atomic_write(
                &directory,
                &base.connectors_manifest,
                to_string_pretty(&doc)
                    .map_err(|_| ApiError::internal())?
                    .as_bytes(),
            )?;
        }
        Ok(())
    }
}

fn validate_id(id: &str) -> ApiResult<()> {
    if id.is_empty()
        || id.len() > 64
        || !id.as_bytes()[0].is_ascii_alphanumeric()
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
        || ["web", "console", "scheduler"].contains(&id)
    {
        return Err(ApiError::invalid(
            "Choose a unique connector ID using 1–64 letters, digits, _ or -.",
        ));
    }
    Ok(())
}
