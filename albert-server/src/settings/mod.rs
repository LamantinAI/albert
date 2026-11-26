//! The HTTP control plane can start without provider credentials or working
//! memory. Invalid agent-only configuration is reported by runtime activation.

use std::{
    env::current_dir,
    fs::read_to_string,
    path::{Path, PathBuf},
};

use albert_connector_web::settings::WebSettings;
use albert_core::{
    config::{config_path, Config},
    error::{Error, Result},
};
use octo_connector_browser::MoliSettings;
use toml::{from_str, Table};

pub(crate) struct ControlSettings {
    pub path: PathBuf,
    pub base: Config,
    pub web: WebSettings,
    pub browser_runtime: MoliSettings,
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
        let base = Config::parse_control_document(&document, &path)?;
        let browser_runtime = browser_settings(&document, &path)?;
        Ok(Self {
            path,
            base,
            web,
            browser_runtime,
        })
    }
}

fn browser_settings(document: &Table, path: &Path) -> Result<MoliSettings> {
    let mut settings: MoliSettings = document
        .get("browser_runtime")
        .cloned()
        .map(|section| section.try_into())
        .transpose()?
        .unwrap_or_default();
    settings.validate().map_err(Error::Config)?;
    let executable = &settings.executable;
    if executable.is_relative() && executable.file_name() != Some(executable.as_os_str()) {
        let directory = path.parent().unwrap_or_else(|| Path::new("."));
        settings.executable = directory.join(executable);
    }
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use std::fs::write;

    use tempfile::tempdir;
    use toml::{from_str, Table};

    use super::{browser_settings, ControlSettings, WebSettings};

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
        ] {
            let doc: Table = from_str(&format!("[web]\n{input}")).unwrap();
            assert!(WebSettings::from_document(&doc).is_err(), "{input}");
        }
        let doc: Table =
            from_str("[web]\nbind='0.0.0.0:9877'\npublic_origin='https://albert.example.org'")
                .unwrap();
        assert!(WebSettings::from_document(&doc).is_ok());
    }

    #[test]
    fn direct_listeners_infer_the_browser_origin_without_an_address_setting() {
        for bind in ["127.0.0.1:9877", "0.0.0.0:9877", "[::]:9877"] {
            let doc: Table = from_str(&format!("[web]\nbind='{bind}'")).unwrap();
            let settings = WebSettings::from_document(&doc).unwrap();
            assert!(settings.public_origin.is_none());
        }
    }

    #[test]
    fn browser_runtime_settings_default_and_resolve_paths() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("albert.toml");
        let defaults = browser_settings(&Table::new(), &path).unwrap();
        assert_eq!(defaults.executable.to_str(), Some("moli"));
        assert_eq!(defaults.bind.to_string(), "127.0.0.1:0");
        assert_eq!(defaults.startup_timeout_secs, 8);

        let document = from_str(
            "[browser_runtime]\nexecutable='bin/moli'\nbind='127.0.0.1:4500'\nstartup_timeout_secs=3",
        )
        .unwrap();
        let settings = browser_settings(&document, &path).unwrap();
        assert_eq!(settings.executable, dir.path().join("bin/moli"));
        assert_eq!(settings.bind.port(), 4500);
        assert_eq!(settings.startup_timeout_secs, 3);
    }

    #[test]
    fn browser_runtime_rejects_nonlocal_bind_and_zero_startup_timeout() {
        for sample in [
            "bind='0.0.0.0:4500'",
            "startup_timeout_secs=0",
            "executable=''",
        ] {
            let document = from_str(&format!("[browser_runtime]\n{sample}")).unwrap();
            assert!(browser_settings(&document, std::path::Path::new("albert.toml")).is_err());
        }
    }
}
