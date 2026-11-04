//! Web listener and authentication settings owned by the HTTP adapter.
use std::{
    fs::read_to_string,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::Path,
};

use albert_core::{
    config::Config,
    error::{Error, Result},
};
use serde::Deserialize;
use toml::{from_str, Table};
use url::Url;

#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WebSettings {
    pub enabled: bool,
    pub bind: SocketAddr,
    pub public_origin: Option<String>,
    pub credential_ttl_seconds: u64,
    pub login_attempts: usize,
    pub login_window_seconds: u64,
    pub password_workers: usize,
    pub max_body_bytes: usize,
}

impl Default for WebSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            bind: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 9877),
            public_origin: None,
            credential_ttl_seconds: 3600,
            login_attempts: 10,
            login_window_seconds: 60,
            password_workers: 2,
            max_body_bytes: 8 * 1024 * 1024,
        }
    }
}

impl WebSettings {
    pub fn from_document(document: &Table) -> Result<Self> {
        let settings: Self = document
            .get("web")
            .cloned()
            .map(|value| value.try_into())
            .transpose()
            .map_err(|_| Error::Config("Invalid web settings.".into()))?
            .unwrap_or_default();
        if !(60..=86400).contains(&settings.credential_ttl_seconds)
            || !(1..=64).contains(&settings.password_workers)
            || settings.login_attempts == 0
            || settings.login_window_seconds == 0
            || !(1024..=16 * 1024 * 1024).contains(&settings.max_body_bytes)
        {
            return Err(Error::Config(
                "Invalid web authentication or body limits.".into(),
            ));
        }
        if let Some(origin) = &settings.public_origin {
            let url = Url::parse(origin)
                .map_err(|_| Error::Config("Invalid web public_origin.".into()))?;
            if !matches!(url.scheme(), "http" | "https")
                || url.origin().ascii_serialization() != *origin
            {
                return Err(Error::Config(
                    "web.public_origin must be an HTTP(S) origin without a path.".into(),
                ));
            }
        }
        Ok(settings)
    }
}

pub fn validate_control_file(path: &Path) -> Result<()> {
    let text = if path.exists() {
        read_to_string(path)?
    } else {
        String::new()
    };
    let document: Table = from_str(&text)?;
    WebSettings::from_document(&document)?;
    Config::parse_control_document(&document, path)?;
    Ok(())
}
