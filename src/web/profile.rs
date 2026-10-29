use std::{fs::read_to_string, path::Path};

use sqlx::query_as;
use toml::{from_str, Table};

use super::Panel;
use crate::{
    api::{
        errors::{ApiError, ApiResult},
        models::{IdentityList, IdentityView, ProfileView},
        principal::Principal,
    },
    manifests::connector_files,
    sessions::{ExternalIdentity, UserId},
};

/// Only the configured private Telegram owner is part of legacy web migration.
/// This is server configuration, never a browser-supplied actor/user identifier.
pub(crate) fn configured_owner(manifest: &Path) -> Option<(String, String)> {
    let mut owners = Vec::new();
    for path in connector_files(manifest) {
        let Ok(text) = read_to_string(path) else {
            continue;
        };
        let Ok(value) = from_str::<Table>(&text) else {
            continue;
        };
        let Some(connector) = value.get("connector") else {
            continue;
        };
        if connector.get("type").and_then(|v| v.as_str()) != Some("telegram") {
            continue;
        }
        if let (Some(id), Some(owner)) = (
            connector.get("id").and_then(|v| v.as_str()),
            connector
                .get("owner_chat")
                .and_then(|v| v.as_integer())
                .filter(|id| *id > 0),
        ) {
            owners.push((id.to_owned(), owner.to_string()));
        }
    }
    owners.sort_by_key(|(id, _)| (id != "telegram", id.clone()));
    owners.into_iter().next()
}

impl Panel {
    pub(super) async fn profile(&self, caller: &Principal) -> ApiResult<ProfileView> {
        if let Some(id) = caller.canonical_user_id() {
            return Ok(ProfileView {
                canonical_user_id: Some(id.into()),
                display_name: Some(caller.account().username.clone()),
            });
        }
        if caller.must_rotate() {
            return Err(ApiError::forbidden());
        }
        let identity = if caller.account_id() == "bootstrap-owner" {
            configured_owner(&self.runtime.base.connectors_manifest)
                .map(|(connector, actor)| ExternalIdentity::new(&connector, &actor))
                .transpose()
                .map_err(|_| ApiError::internal())?
                .unwrap_or(
                    ExternalIdentity::new("web", caller.account_id())
                        .map_err(|_| ApiError::internal())?,
                )
        } else {
            ExternalIdentity::new("web", caller.account_id()).map_err(|_| ApiError::internal())?
        };
        let user = self
            .runtime
            .sessions
            .resolve_identity(&identity)
            .await
            .map_err(|_| ApiError::internal())?;
        match self
            .auth
            .bind_profile(
                caller.account_id(),
                user.as_str(),
                caller.account().revision,
            )
            .await
        {
            Ok(account) => Ok(ProfileView {
                canonical_user_id: account.canonical_user_id,
                display_name: Some(account.username),
            }),
            Err(_) => {
                let current = self.auth.validate_principal(caller).await?;
                if current.canonical_user_id() == Some(user.as_str()) {
                    Ok(ProfileView {
                        canonical_user_id: Some(user.as_str().into()),
                        display_name: Some(current.account().username.clone()),
                    })
                } else {
                    Err(ApiError::conflict("The profile changed. Reload and retry."))
                }
            }
        }
    }

    pub(super) async fn identities(&self, caller: &Principal) -> ApiResult<IdentityList> {
        let user = caller.canonical_user_id().ok_or_else(ApiError::forbidden)?;
        UserId::parse(user).map_err(|_| ApiError::forbidden())?;
        let rows:Vec<(String,String)>=query_as("SELECT connector_id,external_actor_id FROM albert_identity_bindings WHERE user_id=? ORDER BY connector_id,external_actor_id")
            .bind(user).fetch_all(self.runtime.sessions.database().pool()).await.map_err(|_|ApiError::internal())?;
        Ok(IdentityList {
            items: rows
                .into_iter()
                .map(|(connector, external_actor_id)| IdentityView {
                    connector,
                    external_actor_id,
                })
                .collect(),
        })
    }
}
