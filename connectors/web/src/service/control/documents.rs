use super::{redact, ControlService};
use crate::{
    api::errors::ApiError, api::errors::ApiResult, api::models::ConfigDocument,
    service::panel_models::files::hash, service::panel_models::files::read_optional,
    service::panel_models::files::root,
};
use albert_core::config::Config;
use albert_core::manifests::connector_files;
use std::{collections::BTreeMap, fs::read_to_string, path::PathBuf};
use toml::{from_str, Table};
impl ControlService {
    pub(in crate::service) fn documents(&self) -> ApiResult<BTreeMap<String, PathBuf>> {
        let base = &self.runtime.base;
        let mut docs = BTreeMap::from([
            ("albert".into(), self.runtime.path.clone()),
            ("connectors".into(), base.connectors_manifest.clone()),
            (
                "mcp".into(),
                Config::load_control_from(&self.runtime.path)
                    .map(|config| config.mcp_registry)
                    .unwrap_or_else(|_| base.mcp_registry.clone()),
            ),
        ]);
        if let Ok(path) = self.model_path() {
            docs.insert("models".into(), path);
        }
        for path in connector_files(&base.connectors_manifest) {
            let Ok(text) = read_to_string(&path) else {
                continue;
            };
            let Ok(value) = from_str::<Table>(&text) else {
                continue;
            };
            if let Some(id) = value
                .get("connector")
                .and_then(|v| v.get("id"))
                .and_then(|v| v.as_str())
            {
                if id.len() <= 128 {
                    docs.insert(format!("connector:{id}"), path);
                }
            }
        }
        Ok(docs)
    }
    pub(in crate::service) fn model_path(&self) -> ApiResult<PathBuf> {
        self.models.path()
    }
    pub(in crate::service) fn document_path(&self, id: &str) -> ApiResult<PathBuf> {
        if id == "albert" {
            return Ok(self.runtime.path.clone());
        }
        if id == "connectors" {
            return Ok(self.runtime.base.connectors_manifest.clone());
        }
        if id == "models" {
            return self.model_path();
        }
        if id == "soul" {
            return Ok(self.runtime.base.soul_path.clone());
        }
        if id == "system" {
            return Ok(self.runtime.base.system_path.clone());
        }
        if let Some(path) = self.documents()?.remove(id) {
            return Ok(path);
        }
        self.new_connector_path(id)
    }
    pub(crate) fn document(&self, id: &str) -> ApiResult<ConfigDocument> {
        let directory = root(&self.runtime.base.deploy_dir)?;
        let path = self.document_path(id)?;
        let bytes = read_optional(&directory, &path, 1024 * 1024)?.unwrap_or_default();
        let text = String::from_utf8(bytes.clone())
            .map_err(|_| ApiError::invalid("Configuration must be UTF-8."))?;
        let (content, redacted) = redact(text);
        Ok(ConfigDocument {
            path: id.into(),
            content,
            revision: hash(&[&bytes]),
            redacted,
        })
    }
}
