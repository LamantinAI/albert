use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ConfirmationSettings {
    pub request_ttl_seconds: u64,
    pub native_timeout_ms: u64,
    pub notification_ttl_seconds: u64,
    pub tombstone_ttl_seconds: u64,
    pub max_pending: usize,
    pub max_entries: usize,
    pub max_title_bytes: usize,
    pub max_action_bytes: usize,
    pub max_effects: usize,
    pub max_effect_bytes: usize,
    pub max_details_bytes: usize,
}
impl Default for ConfirmationSettings {
    fn default() -> Self {
        Self {
            request_ttl_seconds: 900,
            native_timeout_ms: 10000,
            notification_ttl_seconds: 3600,
            tombstone_ttl_seconds: 300,
            max_pending: 64,
            max_entries: 256,
            max_title_bytes: 256,
            max_action_bytes: 2048,
            max_effects: 32,
            max_effect_bytes: 1024,
            max_details_bytes: 16384,
        }
    }
}
impl ConfirmationSettings {
    pub fn valid(&self) -> bool {
        (1..=120000).contains(&self.native_timeout_ms)
            && (1..=86400).contains(&self.request_ttl_seconds)
            && (1..=86400).contains(&self.notification_ttl_seconds)
            && (1..=86400).contains(&self.tombstone_ttl_seconds)
            && (1..=1024).contains(&self.max_pending)
            && (self.max_pending..=4096).contains(&self.max_entries)
            && (1..=4096).contains(&self.max_title_bytes)
            && (1..=65536).contains(&self.max_action_bytes)
            && (1..=128).contains(&self.max_effects)
            && (1..=65536).contains(&self.max_effect_bytes)
            && (1..=1048576).contains(&self.max_details_bytes)
    }
}
