use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct TelegramLoginSettings {
    pub token_ttl_seconds: u64,
    pub token_retention_seconds: u64,
    pub attempts_per_minute: usize,
    pub issues_per_minute: usize,
    pub max_rate_limit_keys: usize,
}

impl Default for TelegramLoginSettings {
    fn default() -> Self {
        Self {
            token_ttl_seconds: 600,
            token_retention_seconds: 86400,
            attempts_per_minute: 10,
            issues_per_minute: 5,
            max_rate_limit_keys: 4096,
        }
    }
}

impl TelegramLoginSettings {
    pub fn valid(&self) -> bool {
        (60..=3600).contains(&self.token_ttl_seconds)
            && self.token_retention_seconds >= self.token_ttl_seconds
            && self.token_retention_seconds <= 604800
            && self.attempts_per_minute > 0
            && self.issues_per_minute > 0
            && self.max_rate_limit_keys > 0
    }
}
