use serde::{Deserialize, Serialize};

use super::{policy::MessagingLimits, MessagingError, MessagingResult};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct MessagingSettings {
    pub enabled: bool,
    pub limits: MessagingLimits,
    pub poll_ms: u64,
    pub batch_size: usize,
    pub max_concurrent: usize,
    pub max_work_secs: u64,
    pub max_tool_calls: u32,
    pub max_output_bytes: u32,
    pub retention_secs: u64,
    pub delivery_observation_timeout_ms: u64,
    pub unknown_retention_secs: u64,
    pub max_delivery_observations: usize,
    pub inspection_output_bytes: usize,
}
impl Default for MessagingSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            limits: MessagingLimits::default(),
            poll_ms: 200,
            batch_size: 16,
            max_concurrent: 4,
            max_work_secs: 300,
            max_tool_calls: 32,
            max_output_bytes: 65536,
            retention_secs: 86400,
            delivery_observation_timeout_ms: 30_000,
            unknown_retention_secs: 86400,
            max_delivery_observations: 64,
            inspection_output_bytes: 16_384,
        }
    }
}
impl MessagingSettings {
    pub fn validate(&self) -> MessagingResult<()> {
        self.limits.validate()?;
        if !(20..=60_000).contains(&self.poll_ms)
            || !(1..=128).contains(&self.batch_size)
            || !(1..=32).contains(&self.max_concurrent)
            || !(1..=3600).contains(&self.max_work_secs)
            || !(1..=1024).contains(&self.max_tool_calls)
            || !(1..=1_048_576).contains(&self.max_output_bytes)
            || self.retention_secs > 31_536_000
            || !(100..=300_000).contains(&self.delivery_observation_timeout_ms)
            || !(1..=31_536_000).contains(&self.unknown_retention_secs)
            || !(1..=128).contains(&self.max_delivery_observations)
            || !(2048..=65_536).contains(&self.inspection_output_bytes)
        {
            return Err(MessagingError::Invalid(
                "invalid messaging runtime settings",
            ));
        }
        Ok(())
    }
}
