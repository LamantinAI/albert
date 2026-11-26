//! Existing connector configuration and ACL semantics stay in their adapters.
use std::{fs::read_to_string, sync::Arc};

use albert_connector_console::ConsoleConnector;
use albert_connector_telegram::SessionTelegramPolicy;
use albert_core::{
    config::Config,
    error::{Error, Result},
    manifests::connector_files,
    messaging::transport::{AdapterKind, AdapterRegistry},
    service::session::SessionService,
};
use octo_approval::Authority;
use octo_connector_browser::{factory_with_moli as browser_factory, MoliRuntime, MoliSettings};
use octo_connector_caldav::factory as caldav_factory;
use octo_connector_forkd::factory as forkd_factory;
use octo_connector_http::factory as http_factory;
use octo_connector_imagegen::factory as imagegen_factory;
use octo_connector_mail::factory as mail_factory;
use octo_connector_search::factory_with_moli as search_factory;
use octo_connector_speak::factory as speak_factory;
use octo_connector_sql::factory as sql_factory;
use octo_connector_storage::factory as storage_factory;
use octo_connector_telegram::factory_with_policy as telegram_factory;
use octo_connector_transcribe::factory as transcribe_factory;
use octo_core::{Octo, OctoBuilder};
use octo_openai_auth::SubscriptionAuth;
use toml::Value;
use tracing::warn;

use super::report_factory::ReportFactory;
use crate::sql::{console_sql_connectors, protected_paths};

pub(super) fn configured_telegram(config: &Config) -> bool {
    connector_files(&config.connectors_manifest)
        .into_iter()
        .any(|path| {
            let Some(header) = read_to_string(path)
                .ok()
                .and_then(|text| text.parse::<Value>().ok())
            else {
                return false;
            };
            let Some(connector) = header.get("connector") else {
                return false;
            };
            if connector.get("type").and_then(Value::as_str) != Some("telegram") {
                return false;
            }
            let reference = connector
                .get("token_env")
                .and_then(Value::as_str)
                .unwrap_or("OCTO_TELEGRAM_TOKEN");
            config.secret(reference).is_some()
        })
}

pub(super) async fn channels(
    config: &Config,
    sessions: Arc<SessionService>,
    authority: Authority,
    auth: Arc<SubscriptionAuth>,
    has_telegram: bool,
    browser_settings: &MoliSettings,
) -> Result<OctoBuilder> {
    let adapters = sessions.delivery_adapters();
    if has_telegram || config.connectors_manifest.exists() {
        let configured_adapters = AdapterRegistry::default();
        let moli =
            Arc::new(MoliRuntime::with_settings(browser_settings.clone()).map_err(Error::Config)?);
        let configured = Octo::builder()
            .register_connector_type(
                "telegram",
                ReportFactory::new(
                    telegram_factory(
                        authority.clone(),
                        Arc::new(
                            SessionTelegramPolicy::new(sessions.clone(), authority.clone())
                                .with_accounts(sessions.accounts().clone()),
                        ),
                    ),
                    configured_adapters.clone(),
                    AdapterKind::Telegram,
                ),
            )
            .register_connector_type(
                "sql",
                sql_factory(authority.clone(), protected_paths(config)?),
            )
            .register_connector_type("caldav", caldav_factory())
            .register_connector_type("storage", storage_factory())
            .register_connector_type("forkd", forkd_factory())
            .register_connector_type("search", search_factory(moli.clone()))
            .register_connector_type("browser", browser_factory(moli.clone()))
            .register_connector_type("http", http_factory())
            .register_connector_type("mail", mail_factory())
            .register_connector_type("transcribe", transcribe_factory(auth.clone()))
            .register_connector_type("speak", speak_factory(auth.clone()))
            .register_connector_type("imagegen", imagegen_factory(auth))
            .from_config_file(&config.connectors_manifest);
        match configured {
            Ok(builder) => {
                if let Err(error) = moli.start_if_configured().await {
                    warn!(%error, "Moli startup failed, a browser tool call will retry");
                }
                adapters.merge_verified(&configured_adapters);
                return Ok(if has_telegram {
                    builder
                } else {
                    builder.add_connector(console_channel(config, authority, adapters.clone()))
                });
            }
            Err(_) => {
                warn!("Connector configuration could not be loaded; use web settings to repair it.")
            }
        }
    }
    let mut builder = Octo::builder();
    match console_sql_connectors(config, authority.clone()) {
        Ok(connectors) => {
            for connector in connectors {
                builder = builder.add_connector(connector);
            }
        }
        Err(_) => warn!("SQL connector unavailable; use web settings to repair it."),
    }
    Ok(builder.add_connector(console_channel(config, authority, adapters)))
}

fn console_channel(
    config: &Config,
    authority: Authority,
    adapters: AdapterRegistry,
) -> Arc<ConsoleConnector> {
    ConsoleConnector::new("console")
        .with_authority(authority, config.authorization.decision_timeout())
        .with_reports(adapters)
}
