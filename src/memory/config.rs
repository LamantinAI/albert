//! A dedicated memory backend, independent of world-facing connectors.

use std::{collections::HashMap, path::Path};

use serde::Deserialize;
use url::Url;

use super::MemoryError;

#[derive(Clone, Default, Deserialize)]
#[serde(try_from = "RawMemoryConfig")]
pub enum MemoryConfig {
    #[default]
    Embedded,
    Mcp {
        timeout_secs: u64,
        transport: McpTransport,
    },
}

#[derive(Clone)]
pub enum McpTransport {
    Http {
        url: String,
        token_env: Option<String>,
    },
    Stdio {
        command: String,
        args: Vec<String>,
        /// Child variable -> parent variable name. No secret values in TOML.
        env: HashMap<String, String>,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMemoryConfig {
    backend: String,
    transport: Option<String>,
    timeout_secs: Option<u64>,
    url: Option<String>,
    token_env: Option<String>,
    command: Option<String>,
    args: Option<Vec<String>>,
    env: Option<HashMap<String, String>>,
}

impl TryFrom<RawMemoryConfig> for MemoryConfig {
    type Error = String;

    fn try_from(raw: RawMemoryConfig) -> Result<Self, Self::Error> {
        match raw.backend.as_str() {
            "embedded" => {
                if raw.transport.is_some()
                    || raw.timeout_secs.is_some()
                    || raw.url.is_some()
                    || raw.token_env.is_some()
                    || raw.command.is_some()
                    || raw.args.is_some()
                    || raw.env.is_some()
                {
                    return Err("embedded memory does not accept MCP settings".into());
                }
                Ok(Self::Embedded)
            }
            "mcp" => {
                let transport = match raw.transport.as_deref() {
                    Some("http") => {
                        if raw.command.is_some() || raw.args.is_some() || raw.env.is_some() {
                            return Err("HTTP memory does not accept stdio settings".into());
                        }
                        McpTransport::Http {
                            url: raw.url.ok_or("HTTP memory requires url")?,
                            token_env: raw.token_env,
                        }
                    }
                    Some("stdio") => {
                        if raw.url.is_some() || raw.token_env.is_some() {
                            return Err("stdio memory does not accept HTTP settings".into());
                        }
                        McpTransport::Stdio {
                            command: raw.command.ok_or("stdio memory requires command")?,
                            args: raw.args.unwrap_or_default(),
                            env: raw.env.unwrap_or_default(),
                        }
                    }
                    _ => return Err("MCP memory requires transport = http or stdio".into()),
                };
                Ok(Self::Mcp {
                    timeout_secs: raw.timeout_secs.unwrap_or(30),
                    transport,
                })
            }
            _ => Err("memory backend must be embedded or mcp".into()),
        }
    }
}

impl MemoryConfig {
    pub fn validate(&mut self, dir: &Path, has_clouds: bool) -> Result<(), MemoryError> {
        let Self::Mcp {
            timeout_secs,
            transport,
        } = self
        else {
            return Ok(());
        };
        if *timeout_secs == 0 {
            return Err(MemoryError::Config("timeout_secs must be positive".into()));
        }
        if has_clouds {
            return Err(MemoryError::Config(
                "[clouds.*] belongs to embedded memory; configure clouds on the MCP server instead"
                    .into(),
            ));
        }
        match transport {
            McpTransport::Http { url, token_env } => {
                let parsed =
                    Url::parse(url).map_err(|_| MemoryError::Config("invalid MCP URL".into()))?;
                if !matches!(parsed.scheme(), "http" | "https")
                    || parsed.host_str().is_none()
                    || !parsed.username().is_empty()
                    || parsed.password().is_some()
                    || parsed.fragment().is_some()
                {
                    return Err(MemoryError::Config(
                        "MCP URL must be HTTP(S), without credentials or fragment".into(),
                    ));
                }
                if token_env.as_ref().is_some_and(|s| s.trim().is_empty()) {
                    return Err(MemoryError::Config(
                        "token_env must name an environment variable".into(),
                    ));
                }
            }
            McpTransport::Stdio { command, env, .. } => {
                if command.trim().is_empty() {
                    return Err(MemoryError::Config(
                        "stdio command must not be empty".into(),
                    ));
                }
                if command.contains('/') && Path::new(command).is_relative() {
                    *command = dir.join(&*command).to_string_lossy().into_owned();
                }
                if env.iter().any(|(key, source)| {
                    key.is_empty() || key.contains(['=', '\0']) || source.is_empty()
                }) {
                    return Err(MemoryError::Config(
                        "invalid stdio environment mapping".into(),
                    ));
                }
            }
        }
        Ok(())
    }
}
