use albert_core::manifests::connector_files;
use toml_edit::DocumentMut;

use super::{
    connector_fields::{connector_fields, CONNECTOR_TYPES},
    ControlService,
};
use crate::{
    api::{
        errors::ApiResult,
        models::{AccountRole, ConnectorList, ConnectorTypeView, ConnectorView},
        principal::Principal,
    },
    service::panel_models::files::{read_optional, root},
};

impl ControlService {
    pub(crate) fn connector_catalog(&self, caller: &Principal) -> ApiResult<ConnectorList> {
        let owner = caller.role() == AccountRole::Owner && !caller.must_rotate();
        let directory = root(&self.runtime.base.deploy_dir)?;
        let mut items = Vec::new();
        for path in connector_files(&self.runtime.base.connectors_manifest) {
            let Ok(Some(bytes)) = read_optional(&directory, &path, 1024 * 1024) else {
                continue;
            };
            let Ok(text) = String::from_utf8(bytes) else {
                continue;
            };
            let Ok(document) = text.parse::<DocumentMut>() else {
                continue;
            };
            let Some(header) = document.get("connector") else {
                continue;
            };
            let (Some(id), Some(kind)) = (
                header.get("id").and_then(|v| v.as_str()),
                header.get("type").and_then(|v| v.as_str()),
            ) else {
                continue;
            };
            if !owner && kind != "telegram" {
                continue;
            }
            let label = CONNECTOR_TYPES
                .iter()
                .find(|(key, _)| *key == kind)
                .map(|(_, label)| *label)
                .unwrap_or("Custom connector");
            items.push(ConnectorView {
                id: id.into(),
                kind: kind.into(),
                label: label.into(),
                settings_mode: if connector_fields(kind).is_empty() {
                    "raw"
                } else {
                    "structured"
                }
                .into(),
            });
        }
        items.sort_by(|a, b| a.id.cmp(&b.id));
        items.dedup_by(|a, b| a.id == b.id);
        Ok(ConnectorList {
            items,
            available_types: if owner {
                CONNECTOR_TYPES
                    .iter()
                    .map(|(kind, label)| ConnectorTypeView {
                        kind: (*kind).into(),
                        label: (*label).into(),
                    })
                    .collect()
            } else {
                Vec::new()
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        api::models::{AccountRole, ConfigWrite, LoginRequest, UserCreate},
        service::{
            auth::{AuthService, AuthSettings},
            control::fixture::fixture,
        },
    };
    use serde_json::to_string;

    #[tokio::test]
    async fn directory_filters_permissions_and_never_exposes_configuration_or_bindings() {
        let (dir, service, owner) = fixture().await;
        for (id, kind) in [
            ("tg-one", "telegram"),
            ("tg-two", "telegram"),
            ("search", "search"),
        ] {
            let id = format!("connector:{id}");
            let old = service.document(&id).unwrap();
            let content=format!("[connector]\nid={}\ntype={}\ntoken_env='HIDDEN_REFERENCE'\ndescription='PRIVATE_CONFIG_SENTINEL'\n", to_string(id.trim_start_matches("connector:")).unwrap(),to_string(kind).unwrap());
            service
                .put(
                    &id,
                    ConfigWrite {
                        path: id.clone(),
                        content,
                        revision: Some(old.revision),
                    },
                )
                .await
                .unwrap();
        }
        let full = service.connector_catalog(&owner).unwrap();
        assert_eq!(full.items.len(), 3);
        assert!(full.available_types.iter().any(|t| t.kind == "storage"));
        let text = to_string(&full).unwrap();
        for hidden in [
            "HIDDEN_REFERENCE",
            "PRIVATE_CONFIG_SENTINEL",
            "token_env",
            "bindings",
            "/tmp/",
        ] {
            assert!(!text.contains(hidden));
        }
        let auth = AuthService::open(
            service.runtime.sessions.database().pool().clone(),
            dir.path().join("catalog.key"),
            AuthSettings::default(),
        )
        .await
        .unwrap();
        auth.create_user(
            &owner.owner_permit().unwrap(),
            UserCreate {
                username: "catalog-user".into(),
                password: "fixture-user-password".into(),
                role: AccountRole::User,
            },
        )
        .await
        .unwrap();
        let login = auth
            .login(LoginRequest {
                username: "catalog-user".into(),
                password: "fixture-user-password".into(),
            })
            .await
            .unwrap();
        let user = auth.authenticate(&login.token).await.unwrap();
        let own = service.connector_catalog(&user).unwrap();
        assert_eq!(own.items.len(), 2);
        assert!(own.items.iter().all(|item| item.kind == "telegram"));
        assert!(own.available_types.is_empty());
    }
}
