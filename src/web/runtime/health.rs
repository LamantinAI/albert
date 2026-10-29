//! Report requested connector activation independently of a healthy model.
use std::fs::read_to_string;

use toml::{from_str, Table};

use super::AgentRuntime;
use crate::manifests::connector_files;

impl AgentRuntime {
    pub(crate) fn connector_error(&self, document: Option<&str>) -> Option<String> {
        let context = self.context()?;
        if let Some(id) = document.and_then(|id| id.strip_prefix("connector:")) {
            return (!context
                .connectors()
                .iter()
                .any(|entry| entry.id.as_str() == id))
            .then(|| {
                format!("Connector {id} did not load. Review its configuration and credentials.")
            });
        }
        if document.is_some_and(|id| id != "connectors" && id != "albert") {
            return None;
        }
        for path in connector_files(&self.base.connectors_manifest) {
            if !path.is_file() {
                continue;
            }
            let header = read_to_string(path)
                .ok()
                .and_then(|text| from_str::<Table>(&text).ok());
            let id = header
                .as_ref()
                .and_then(|value| value.get("connector"))
                .and_then(|value| value.get("id"))
                .and_then(|value| value.as_str());
            match id {
                Some(id)
                    if context
                        .connectors()
                        .iter()
                        .any(|entry| entry.id.as_str() == id) => {}
                Some(id) => {
                    return Some(format!(
                        "Connector {id} did not load. Review its configuration and credentials."
                    ))
                }
                None => {
                    return Some(
                        "A connector manifest could not be loaded. Review connector configuration."
                            .into(),
                    )
                }
            }
        }
        None
    }
}
