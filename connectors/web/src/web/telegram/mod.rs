use std::{collections::BTreeSet, fs::read_to_string};

use albert_core::{accounts::AccountError, manifests::connector_files};
use toml_edit::DocumentMut;

use super::Panel;
use crate::api::{
    errors::{ApiError, ApiResult},
    host::{HostResponse, TelegramOperation},
    models::{
        EmptyObject, TelegramBindingView, TelegramConnectionList, TelegramConnectionView,
        TelegramCredentialWrite, TelegramLoginTokenView,
    },
    principal::Principal,
};

fn account_error(error: AccountError) -> ApiError {
    match error {
        AccountError::Denied => ApiError::forbidden(),
        AccountError::BotUnavailable => {
            ApiError::conflict("The bot is not connected yet. Ask the owner to configure Telegram.")
        }
        AccountError::RateLimited => {
            ApiError::invalid("Too many attempts. Please try again later.")
        }
        _ => ApiError::internal(),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs::read_to_string,
        sync::{atomic::AtomicBool, Arc},
    };

    use albert_core::{
        config::Config,
        manifests::connector_files,
        runtime::AgentRuntime,
        service::session::SessionService,
        sessions::{settings::SessionSettings, transport::LocalTransport},
    };
    use octo_history::SqliteHistory;
    use serde_json::to_string;
    use tempfile::{tempdir, TempDir};
    use toml_edit::DocumentMut;

    use super::Panel;
    use crate::{
        api::{
            host::{HostResponse, TelegramOperation},
            models::{
                AccountRole, CredentialUpdate, LoginRequest, TelegramCredentialWrite, UserCreate,
                UserUpdate,
            },
            principal::Principal,
        },
        service::auth::{AuthService, AuthSettings},
    };

    async fn fixture() -> (TempDir, Arc<Panel>, Principal) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("albert.toml");
        let config = Config::load_control_from(&path).unwrap();
        let sessions = Arc::new(
            SessionService::from_history(
                Arc::new(
                    SqliteHistory::open_retained(dir.path().join("history.db"))
                        .await
                        .unwrap(),
                ),
                SessionSettings::default(),
            )
            .await
            .unwrap(),
        );
        let auth = AuthService::open(
            sessions.database().pool().clone(),
            dir.path().join("key"),
            AuthSettings::default(),
        )
        .await
        .unwrap();
        let login = auth
            .login(LoginRequest {
                username: "owner".into(),
                password: "owner".into(),
            })
            .await
            .unwrap();
        let initial = auth.authenticate(&login.token).await.unwrap();
        let (_, login) = auth
            .rotate_credentials(
                &initial,
                CredentialUpdate {
                    current_password: "owner".into(),
                    new_password: "rotated-owner-password".into(),
                    new_username: None,
                },
            )
            .await
            .unwrap();
        let caller = auth.authenticate(&login.token).await.unwrap();
        let authority = config
            .authorization
            .build()
            .with_scope_validator(sessions.scope_registry());
        let runtime = AgentRuntime::new(
            path,
            config,
            sessions,
            authority,
            Arc::new(AtomicBool::new(false)),
            Arc::new(LocalTransport),
        );
        let panel = Panel::new(runtime, auth.clone());
        panel.profile(&caller).await.unwrap();
        let caller = auth.authenticate(&login.token).await.unwrap();
        (dir, panel, caller)
    }

    #[tokio::test]
    async fn connector_setup_is_write_only_and_self_tokens_need_no_model() {
        let (_dir, panel, owner) = fixture().await;
        assert!(!panel.runtime.status().model_ready);
        let credential = "123456789:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        let result = panel
            .telegram_operation(
                &owner,
                TelegramOperation::Credential {
                    id: "telegram".into(),
                    input: TelegramCredentialWrite {
                        bot_token: credential.into(),
                    },
                },
            )
            .await
            .unwrap();
        let HostResponse::TelegramConnection(status) = result else {
            panic!("Expected connection");
        };
        assert!(!status.connected);
        assert!(status.requires_restart);
        let manifest = read_to_string(
            connector_files(&panel.runtime.base.connectors_manifest)
                .into_iter()
                .find(|path| path.file_name().is_some_and(|name| name == "telegram.toml"))
                .unwrap(),
        )
        .unwrap();
        assert!(manifest.contains("token_env"));
        assert!(!manifest.contains(credential));
        assert!(!manifest.contains("owner_chat"));
        assert!(!to_string(&status).unwrap().contains(credential));
        assert!(panel
            .telegram_operation(
                &owner,
                TelegramOperation::Issue {
                    id: "telegram".into()
                }
            )
            .await
            .is_err());
        panel
            .runtime
            .sessions
            .accounts()
            .register_bot("telegram", 99, "fixture_bot")
            .await
            .unwrap();
        let HostResponse::TelegramToken(token) = panel
            .telegram_operation(
                &owner,
                TelegramOperation::Issue {
                    id: "telegram".into(),
                },
            )
            .await
            .unwrap()
        else {
            panic!("Expected token");
        };
        panel
            .runtime
            .sessions
            .accounts()
            .login("telegram", 99, 7, 7, &token.token)
            .await
            .unwrap();
        let HostResponse::TelegramConnection(status) = panel
            .telegram_operation(
                &owner,
                TelegramOperation::Status {
                    id: "telegram".into(),
                },
            )
            .await
            .unwrap()
        else {
            panic!("Expected status");
        };
        assert!(status.connected);
        assert_eq!(status.bindings[0].actor_id, "7");
        let grant = panel
            .runtime
            .sessions
            .accounts()
            .actor("telegram", 99, 7)
            .await
            .unwrap()
            .unwrap();
        panel
            .telegram_operation(
                &owner,
                TelegramOperation::Disconnect {
                    id: "telegram".into(),
                },
            )
            .await
            .unwrap();
        assert!(!grant.valid().await);
        assert!(
            panel
                .runtime
                .sessions
                .accounts()
                .account(owner.account_id())
                .await
                .unwrap()
                .valid()
                .await
        );
    }

    #[tokio::test]
    async fn users_only_see_own_binding_and_cannot_set_bot_credentials() {
        let (_dir, panel, owner) = fixture().await;
        panel
            .telegram_operation(
                &owner,
                TelegramOperation::Credential {
                    id: "telegram".into(),
                    input: TelegramCredentialWrite {
                        bot_token: "123456789:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into(),
                    },
                },
            )
            .await
            .unwrap();
        panel
            .runtime
            .sessions
            .accounts()
            .register_bot("telegram", 99, "fixture_bot")
            .await
            .unwrap();
        let user = panel
            .auth
            .create_user(
                &owner.owner_permit().unwrap(),
                UserCreate {
                    username: "registered".into(),
                    password: "initial-user-password".into(),
                    role: AccountRole::User,
                },
            )
            .await
            .unwrap();
        let login = panel
            .auth
            .login(LoginRequest {
                username: user.username,
                password: "initial-user-password".into(),
            })
            .await
            .unwrap();
        let initial = panel.auth.authenticate(&login.token).await.unwrap();
        let (_, login) = panel
            .auth
            .rotate_credentials(
                &initial,
                CredentialUpdate {
                    current_password: "initial-user-password".into(),
                    new_password: "rotated-user-password".into(),
                    new_username: None,
                },
            )
            .await
            .unwrap();
        let user = panel.auth.authenticate(&login.token).await.unwrap();
        assert!(panel
            .telegram_operation(
                &user,
                TelegramOperation::Credential {
                    id: "telegram".into(),
                    input: TelegramCredentialWrite {
                        bot_token: "unused".into()
                    }
                }
            )
            .await
            .is_err());
        let HostResponse::TelegramToken(token) = panel
            .telegram_operation(
                &user,
                TelegramOperation::Issue {
                    id: "telegram".into(),
                },
            )
            .await
            .unwrap()
        else {
            panic!("Expected token");
        };
        panel
            .runtime
            .sessions
            .accounts()
            .login("telegram", 99, 8, 8, &token.token)
            .await
            .unwrap();
        let HostResponse::TelegramConnection(owner_status) = panel
            .telegram_operation(
                &owner,
                TelegramOperation::Status {
                    id: "telegram".into(),
                },
            )
            .await
            .unwrap()
        else {
            panic!("Expected status");
        };
        assert!(owner_status.bindings.is_empty());
        panel
            .telegram_operation(
                &owner,
                TelegramOperation::Disconnect {
                    id: "telegram".into(),
                },
            )
            .await
            .unwrap();
        assert!(panel
            .runtime
            .sessions
            .accounts()
            .actor("telegram", 99, 8)
            .await
            .unwrap()
            .is_some());
        let grant = panel
            .runtime
            .sessions
            .accounts()
            .actor("telegram", 99, 8)
            .await
            .unwrap()
            .unwrap();
        panel
            .auth
            .update_user(
                &owner.owner_permit().unwrap(),
                user.account_id(),
                UserUpdate {
                    revision: user.account().revision,
                    role: None,
                    disabled: Some(true),
                },
            )
            .await
            .unwrap();
        assert!(!grant.is_live());
        assert!(panel
            .runtime
            .sessions
            .accounts()
            .actor("telegram", 99, 8)
            .await
            .unwrap()
            .is_none());
    }
    #[tokio::test]
    async fn similarly_named_connectors_keep_independent_credential_references() {
        let (_dir, panel, owner) = fixture().await;
        let permit = owner.owner_permit().unwrap();
        let first = panel
            .control
            .telegram_credential(
                &permit,
                "alt-bot",
                "123456789:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            )
            .await
            .unwrap();
        let second = panel
            .control
            .telegram_credential(
                &permit,
                "alt_bot",
                "987654321:BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
            )
            .await
            .unwrap();
        let first = first.content.parse::<DocumentMut>().unwrap();
        let second = second.content.parse::<DocumentMut>().unwrap();
        assert_ne!(
            first["connector"]["token_env"].as_str(),
            second["connector"]["token_env"].as_str()
        );
    }
}

impl Panel {
    fn telegram_instances(&self) -> BTreeSet<String> {
        connector_files(&self.runtime.base.connectors_manifest)
            .into_iter()
            .filter_map(|path| {
                let text = read_to_string(path).ok()?;
                let document = text.parse::<DocumentMut>().ok()?;
                let connector = document.get("connector")?;
                (connector.get("type")?.as_str() == Some("telegram"))
                    .then(|| {
                        connector
                            .get("id")
                            .and_then(|id| id.as_str())
                            .map(str::to_owned)
                    })
                    .flatten()
            })
            .collect()
    }

    async fn telegram_view(
        &self,
        caller: &Principal,
        connector: &str,
    ) -> ApiResult<TelegramConnectionView> {
        if !self.telegram_instances().contains(connector) {
            return Err(ApiError::not_found());
        }
        let accounts = self.runtime.sessions.accounts();
        let bot = accounts.bot(connector);
        let bindings = accounts
            .bindings(caller.account_id(), connector)
            .await
            .map_err(account_error)?;
        Ok(TelegramConnectionView {
            connector_id: connector.into(),
            connected: bot.is_some(),
            bot_username: bot.map(|b| b.username),
            requires_restart: accounts.requires_restart(connector),
            bindings: bindings
                .into_iter()
                .map(|b| TelegramBindingView {
                    actor_id: b.actor_id,
                    chat_id: b.chat_id,
                    created_at: b.created_at,
                })
                .collect(),
        })
    }

    async fn telegram_credential(
        &self,
        caller: &Principal,
        connector: &str,
        input: TelegramCredentialWrite,
    ) -> ApiResult<TelegramConnectionView> {
        let permit = caller.owner_permit()?;
        if connector.is_empty()
            || connector.len() > 64
            || !connector
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(ApiError::invalid("Choose a valid connector name."));
        }
        let token = input.bot_token.trim();
        if token.is_empty()
            || token.len() > 256
            || token.chars().any(char::is_whitespace)
            || !token.contains(':')
        {
            return Err(ApiError::invalid("Enter the bot token from BotFather."));
        }
        self.control
            .telegram_credential(&permit, connector, token)
            .await?;
        self.runtime.sessions.accounts().mark_restarting(connector);
        let mut status = self.telegram_view(caller, connector).await?;
        status.requires_restart = true;
        Ok(status)
    }

    pub(super) async fn telegram_operation(
        &self,
        caller: &Principal,
        operation: TelegramOperation,
    ) -> ApiResult<HostResponse> {
        // All self operations derive the account from the already verified web
        // principal. No body can nominate a different user or inherit a role.
        match operation {
            TelegramOperation::List => {
                let mut items = Vec::new();
                for id in self.telegram_instances() {
                    items.push(self.telegram_view(caller, &id).await?);
                }
                Ok(HostResponse::TelegramConnections(TelegramConnectionList {
                    items,
                }))
            }
            TelegramOperation::Status { id } => Ok(HostResponse::TelegramConnection(
                self.telegram_view(caller, &id).await?,
            )),
            TelegramOperation::Credential { id, input } => Ok(HostResponse::TelegramConnection(
                self.telegram_credential(caller, &id, input).await?,
            )),
            TelegramOperation::Issue { id } => {
                if !self.telegram_instances().contains(&id) {
                    return Err(ApiError::not_found());
                }
                let token = self
                    .runtime
                    .sessions
                    .accounts()
                    .issue(caller.account_id(), &id)
                    .await
                    .map_err(account_error)?;
                Ok(HostResponse::TelegramToken(TelegramLoginTokenView {
                    id: token.id,
                    token: token.token,
                    expires_at: token.expires_at,
                }))
            }
            TelegramOperation::RevokeToken { id } => {
                self.runtime
                    .sessions
                    .accounts()
                    .revoke_token(caller.account_id(), &id)
                    .await
                    .map_err(account_error)?;
                Ok(HostResponse::Empty(EmptyObject {}))
            }
            TelegramOperation::Disconnect { id } => {
                self.runtime
                    .sessions
                    .accounts()
                    .disconnect(caller.account_id(), &id)
                    .await
                    .map_err(account_error)?;
                Ok(HostResponse::TelegramConnection(
                    self.telegram_view(caller, &id).await?,
                ))
            }
        }
    }
}
