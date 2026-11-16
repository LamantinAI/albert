//! Private initialization input. Credential values are captured once and never
//! enter configuration documents, memory tools, DTOs or diagnostic output.
use std::{collections::HashMap, path::PathBuf};

use serde_json::{json, to_vec, Value};
use sha2::{Digest, Sha256};

use super::config::{McpTransport, MemoryConfig};
use crate::config::Config;

pub(super) struct Initialization {
    pub memory: MemoryConfig,
    pub vault: PathBuf,
    pub credentials: HashMap<String, String>,
    pub fingerprint: String,
}
impl Initialization {
    pub fn capture(config: &Config, vault: Option<PathBuf>) -> Self {
        let vault = vault.unwrap_or_else(|| config.embedded_vault_path());
        let mut references = Vec::new();
        match &config.memory {
            MemoryConfig::Mcp {
                transport: McpTransport::Http { token_env, .. },
                ..
            } => references.extend(token_env.iter().cloned()),
            MemoryConfig::Mcp {
                transport: McpTransport::Stdio { env, .. },
                ..
            } => references.extend(env.values().cloned()),
            MemoryConfig::Embedded => {
                references.extend(config.clouds.values().map(|cloud| cloud.token_env.clone()))
            }
        }
        let credentials: HashMap<_, _> = references
            .into_iter()
            .filter_map(|name| config.secret(&name).map(|value| (name, value)))
            .collect();
        let hashes: HashMap<_, _> = credentials
            .iter()
            .map(|(name, value)| {
                (
                    name.clone(),
                    format!("{:x}", Sha256::digest(value.as_bytes())),
                )
            })
            .collect();
        let embedded = matches!(config.memory, MemoryConfig::Embedded);
        let mut identity = json!({"memory":config.memory,"vault":if embedded {Some(&vault)} else {None},
            "clouds":if embedded {Some(&config.clouds)} else {None},
            "clouds_default":if embedded {config.clouds_default.as_ref()} else {None},"credentials":hashes});
        canonical(&mut identity);
        let fingerprint = format!(
            "{:x}",
            Sha256::digest(to_vec(&identity).expect("memory settings serialize"))
        );
        Self {
            memory: config.memory.clone(),
            vault,
            credentials,
            fingerprint,
        }
    }
}
fn canonical(value: &mut Value) {
    match value {
        Value::Object(values) => {
            values.sort_keys();
            for value in values.values_mut() {
                canonical(value);
            }
        }
        Value::Array(values) => {
            for value in values {
                canonical(value);
            }
        }
        _ => {}
    }
}
