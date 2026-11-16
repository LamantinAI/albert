//! Deployment-wide child concurrency and retention limits.
use super::inspection::Settings as InspectionSettings;
use serde::Deserialize;
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub enabled: bool,
    pub inspection: InspectionSettings,
    pub max_concurrent: usize,
    pub max_retained: usize,
    pub max_runs_per_turn: usize,
    pub max_tool_turns: usize,
    pub default_tool_turns: usize,
    pub timeout_secs: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            enabled: true,
            inspection: Default::default(),
            max_concurrent: 4,
            max_retained: 64,
            max_runs_per_turn: 8,
            max_tool_turns: 17,
            default_tool_turns: 11,
            timeout_secs: 600,
        }
    }
}
impl Limits {
    pub fn validate(&self) -> Result<(), String> {
        if self.max_concurrent == 0
            || self.max_retained < self.max_concurrent
            || self.max_runs_per_turn == 0
            || self.max_tool_turns == 0
            || self.default_tool_turns == 0
            || self.timeout_secs == 0
            || !self.inspection.validate()
        {
            return Err(
                "subagents: limits must be positive; max_retained >= max_concurrent".into(),
            );
        }
        Ok(())
    }
}
