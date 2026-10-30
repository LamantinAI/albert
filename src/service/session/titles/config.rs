use serde::{Deserialize, Serialize};

use crate::sessions::error::{SessionError, SessionResult};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct TitleSettings {
    pub enabled: bool,
    pub max_concurrent: usize,
    pub input_bytes: usize,
    pub scan_records: usize,
    pub title_chars: usize,
    pub output_tokens: usize,
    pub timeout_secs: u64,
    pub max_attempts: usize,
}
impl Default for TitleSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            max_concurrent: 2,
            input_bytes: 1024,
            scan_records: 16,
            title_chars: 64,
            output_tokens: 1024,
            timeout_secs: 15,
            max_attempts: 1,
        }
    }
}
impl TitleSettings {
    pub(crate) fn validate(&self) -> SessionResult<()> {
        if !(1..=64).contains(&self.scan_records)
            || !(1..=8).contains(&self.max_concurrent)
            || !(128..=4096).contains(&self.input_bytes)
            || !(16..=128).contains(&self.title_chars)
            || !(16..=4096).contains(&self.output_tokens)
            || !(1..=60).contains(&self.timeout_secs)
            || !(1..=3).contains(&self.max_attempts)
        {
            return Err(SessionError::InvalidInput(
                "invalid title generation settings",
            ));
        }
        Ok(())
    }
}
