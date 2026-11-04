use albert_core::selfconfig::SelfConfig;
use toml_edit::{value, DocumentMut, Item, Table};

use super::ControlService;
use crate::{
    api::{
        errors::{ApiError, ApiResult},
        models::{ConfigDocument, ConfigWrite},
        principal::OwnerPermit,
    },
    service::panel_models::files::hash,
};

impl ControlService {
    pub(crate) async fn telegram_credential(
        &self,
        _permit: &OwnerPermit,
        connector: &str,
        token: &str,
    ) -> ApiResult<ConfigDocument> {
        let _guard = self.gate.lock().await;
        let id = format!("connector:{connector}");
        let old = self.document(&id)?;
        let mut document = old
            .content
            .parse::<DocumentMut>()
            .map_err(|_| ApiError::invalid("The connector configuration could not be read."))?;
        if document.get("connector").is_none() {
            document["connector"] = Item::Table(Table::new());
        }
        if !document["connector"].is_table() {
            return Err(ApiError::invalid("A connector table is required."));
        }
        if document["connector"]
            .get("type")
            .and_then(Item::as_str)
            .is_some_and(|kind| kind != "telegram")
        {
            return Err(ApiError::invalid("That name belongs to another connector."));
        }
        // A connector-specific reference avoids overwriting unrelated secrets.
        let env = format!(
            "ALBERT_TELEGRAM_{}_{}_TOKEN",
            connector.replace('-', "_").to_ascii_uppercase(),
            hash(&[connector.as_bytes()])[..16].to_ascii_uppercase()
        );
        document["connector"]["id"] = value(connector);
        document["connector"]["type"] = value("telegram");
        document["connector"]["token_env"] = value(&env);
        if let Some(table) = document["connector"].as_table_like_mut() {
            for key in ["owner_chat", "admins", "acl_path", "token", "bot_token"] {
                table.remove(key);
            }
        }
        let input = ConfigWrite {
            path: id.clone(),
            content: document.to_string(),
            revision: Some(old.revision),
        };
        let validation = self.validate(&input);
        if !validation.valid {
            return Err(ApiError::invalid(validation.issues.join(" ")));
        }
        SelfConfig::new(self.runtime.base.deploy_dir.clone())
            .panel_set_secret(&env, token)
            .map_err(|_| ApiError::invalid("The bot credential could not be saved."))?;
        self.put_locked(&id, input)
    }
}
