use serde::{Deserialize, Serialize};

use crate::sessions::error::{SessionError, SessionResult};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct PickerSettings {
    pub page_size: usize,
    pub ttl_secs: u64,
    pub max_lists: usize,
    pub max_receipts_per_list: usize,
}
impl Default for PickerSettings {
    fn default() -> Self {
        Self {
            page_size: 5,
            ttl_secs: 600,
            max_lists: 128,
            max_receipts_per_list: 64,
        }
    }
}
impl PickerSettings {
    pub(crate) fn validate(&self) -> SessionResult<()> {
        if !(1..=10).contains(&self.page_size)
            || !(1..=3600).contains(&self.ttl_secs)
            || !(1..=256).contains(&self.max_lists)
            || !(1..=128).contains(&self.max_receipts_per_list)
        {
            return Err(SessionError::InvalidInput("invalid session picker limits"));
        }
        Ok(())
    }
}
