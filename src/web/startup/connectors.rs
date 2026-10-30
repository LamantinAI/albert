//! Existing connector configuration and ACL semantics stay in their adapters.
use std::sync::Arc;

use octo_approval::Authority;
use octo_connector_browser::factory as browser_factory;
use octo_connector_caldav::factory as caldav_factory;
use octo_connector_forkd::factory as forkd_factory;
use octo_connector_http::factory as http_factory;
use octo_connector_imagegen::factory as imagegen_factory;
use octo_connector_mail::factory as mail_factory;
use octo_connector_search::factory as search_factory;
use octo_connector_speak::factory as speak_factory;
use octo_connector_sql::factory as sql_factory;
use octo_connector_storage::factory as storage_factory;
use octo_connector_telegram::factory_with_policy as telegram_factory;
use octo_connector_transcribe::factory as transcribe_factory;
use octo_core::{Octo, OctoBuilder};
use octo_openai_auth::SubscriptionAuth;
use tracing::warn;

use crate::{
    config::Config,
    console::ConsoleConnector,
    error::Result,
    messaging::transport::{AdapterKind, AdapterRegistry, ReportFactory},
    service::session::SessionService,
    sessions::telegram::SessionTelegramPolicy,
    sql::{console_sql_connectors, protected_paths},
};

pub(super) fn channels(
    config: &Config,
    sessions: Arc<SessionService>,
    authority: Authority,
    auth: Arc<SubscriptionAuth>,
    has_telegram: bool,
) -> Result<OctoBuilder> {
    let adapters = sessions.delivery_adapters();
    if has_telegram || config.connectors_manifest.exists() {
        let configured_adapters = AdapterRegistry::default();
        let configured = Octo::builder()
            .register_connector_type(
                "telegram",
                ReportFactory::new(
                    telegram_factory(
                        authority.clone(),
                        Arc::new(SessionTelegramPolicy::new(
                            sessions.clone(),
                            authority.clone(),
                        )),
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
            .register_connector_type("search", search_factory())
            .register_connector_type("browser", browser_factory())
            .register_connector_type("http", http_factory())
            .register_connector_type("mail", mail_factory())
            .register_connector_type("transcribe", transcribe_factory(auth.clone()))
            .register_connector_type("speak", speak_factory(auth.clone()))
            .register_connector_type("imagegen", imagegen_factory(auth))
            .from_config_file(&config.connectors_manifest);
        match configured {
            Ok(builder) => {
                adapters.merge_verified(&configured_adapters);
                return Ok(if has_telegram {
                    builder
                } else {
                    builder.add_connector(
                        ConsoleConnector::new("console")
                            .with_authority(
                                authority.clone(),
                                config.authorization.decision_timeout(),
                            )
                            .with_reports(adapters.clone()),
                    )
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
    Ok(builder.add_connector(
        ConsoleConnector::new("console")
            .with_authority(authority, config.authorization.decision_timeout())
            .with_reports(adapters),
    ))
}
