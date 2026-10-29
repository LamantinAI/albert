//! The HTTP control plane can start without provider credentials or working
//! memory. Invalid agent-only configuration is reported by runtime activation.

use std::{
    env::current_dir,
    fs::read_to_string,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
};

use serde::Deserialize;
use toml::{from_str, to_string, Table};
use url::Url;

use super::{config_path, Config};
use crate::error::{Error, Result};

#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct WebSettings {
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

pub(crate) struct ControlSettings {
    pub path: PathBuf,
    pub base: Config,
    pub web: WebSettings,
}

impl ControlSettings {
    pub fn load() -> Result<Self> {
        Self::from_path(&config_path())
    }

    pub fn from_path(path: &Path) -> Result<Self> {
        let path = if path.is_absolute() {
            path.to_owned()
        } else {
            current_dir()?.join(path)
        };
        let text = if path.exists() {
            read_to_string(&path)?
        } else {
            String::new()
        };
        let document: Table = from_str(&text)?;
        let web = WebSettings::from_document(&document)?;
        let mut core = Table::new();
        for key in [
            "history",
            "sessions",
            "authorization",
            "scheduler",
            "prompt",
            "skills",
            "code",
            "timezone",
            "subscription",
            "connectors_manifest",
        ] {
            if let Some(value) = document.get(key) {
                core.insert(key.into(), value.clone());
            }
        }
        let base = Config::parse_text(
            &to_string(&core).map_err(|_| Error::Config("Invalid control settings.".into()))?,
            &path,
        )?;
        Ok(Self { path, base, web })
    }
}

impl WebSettings {
    pub(crate) fn from_document(document: &Table) -> Result<Self> {
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
        } else if settings.enabled && !settings.bind.ip().is_loopback() {
            return Err(Error::Config(
                "Set web.public_origin when binding to a non-loopback address.".into(),
            ));
        }
        Ok(settings)
    }
}

#[cfg(test)]
mod tests {
    use super::{ControlSettings, WebSettings};
    use std::fs::write;
    use tempfile::tempdir;
    use toml::{from_str, Table};

    #[test]
    fn unconfigured_or_invalid_provider_does_not_prevent_control_startup() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("albert.toml");
        let fresh = ControlSettings::from_path(&path).unwrap();
        assert!(fresh.web.enabled);
        assert!(fresh.base.model.is_empty());
        write(
            &path,
            "model_pool='missing.toml'\n[memory]\nbackend='bad'\n",
        )
        .unwrap();
        assert!(ControlSettings::from_path(&path).is_ok());
    }
    #[test]
    fn web_validation_rejects_listener_auth_and_origin_errors_before_saving() {
        for input in [
            "bind='bad'",
            "credential_ttl_seconds=1",
            "password_workers=0",
            "typo=true",
            "max_body_bytes=0",
            "public_origin='https://example.org/path'",
            "bind='0.0.0.0:9877'",
        ] {
            let doc: Table = from_str(&format!("[web]\n{input}")).unwrap();
            assert!(WebSettings::from_document(&doc).is_err(), "{input}");
        }
        let doc: Table =
            from_str("[web]\nbind='0.0.0.0:9877'\npublic_origin='https://albert.example.org'")
                .unwrap();
        assert!(WebSettings::from_document(&doc).is_ok());
    }
}
