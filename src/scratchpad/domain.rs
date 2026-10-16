use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct PadScope {
    pub conversation: String,
    pub owner: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Assignment {
    pub role: Option<String>,
    #[serde(default)]
    pub models: Vec<String>,
    pub reasoning_effort: Option<String>,
    pub why: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct StageSpec {
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub assignment: Assignment,
    #[serde(default)]
    pub acceptance: Vec<String>,
    #[serde(default)]
    pub scope: Vec<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pending,
    InProgress,
    Done,
    Verified,
    Blocked,
}
impl Status {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "pending" => Some(Self::Pending),
            "in_progress" | "in-progress" | "in progress" => Some(Self::InProgress),
            "done" => Some(Self::Done),
            "verified" => Some(Self::Verified),
            "blocked" => Some(Self::Blocked),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Done => "done",
            Self::Verified => "verified",
            Self::Blocked => "blocked",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub criterion: usize,
    pub reference: String,
    pub summary: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Stage {
    pub id: String,
    pub text: String,
    pub status: Status,
    pub note: Option<String>,
    pub spec: Option<StageSpec>,
    pub attempts: Vec<StageAttempt>,
    pub evidence: Vec<Evidence>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Scratchpad {
    pub task_id: String,
    pub revision: u64,
    pub goal: Option<String>,
    pub steps: Vec<Stage>,
    pub notes: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageLink {
    pub task_id: String,
    pub stage_id: String,
    pub expected_revision: u64,
    pub launch_key: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedAssignment {
    pub role: Option<String>,
    pub models: Vec<String>,
    pub reasoning_effort: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptState {
    Reserved,
    Running,
    Completed,
    Partial,
    Failed,
    Cancelled,
    TimedOut,
}
impl AttemptState {
    pub fn is_live(self) -> bool {
        matches!(self, Self::Reserved | Self::Running)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StageAttempt {
    pub id: String,
    pub launch_key: String,
    pub requested_revision: u64,
    pub assignment: ResolvedAssignment,
    pub run_id: Option<String>,
    pub state: AttemptState,
    pub actual_models: Vec<String>,
    pub outcome_summary: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub enum Reservation {
    New {
        attempt_id: String,
    },
    Existing {
        attempt_id: String,
        run_id: Option<String>,
        state: AttemptState,
    },
}
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub pads: usize,
    pub stages: usize,
    pub notes: usize,
    pub attempts: usize,
    pub text_bytes: usize,
    pub render_bytes: usize,
    pub page_size: usize,
    pub page_bytes: usize,
    pub pad_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            pads: 128,
            stages: 128,
            notes: 128,
            attempts: 32,
            text_bytes: 16_384,
            render_bytes: 16_384,
            page_size: 32,
            page_bytes: 65_536,
            pad_bytes: 1_048_576,
        }
    }
}
impl Limits {
    pub fn validate(&self) -> Result<(), String> {
        if [
            self.pads,
            self.stages,
            self.notes,
            self.attempts,
            self.text_bytes,
            self.render_bytes,
            self.page_size,
            self.page_bytes,
            self.pad_bytes,
        ]
        .contains(&0)
        {
            return Err("scratchpad limits must be positive".into());
        }
        Ok(())
    }
}
pub type Settings = Limits;
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PlanArgs {
    pub action: String,
    pub task_id: Option<String>,
    pub expected_revision: Option<u64>,
    pub stage_id: Option<String>,
    pub text: Option<String>,
    pub spec: Option<StageSpec>,
    pub status: Option<String>,
    pub note: Option<String>,
    pub evidence: Option<Vec<Evidence>>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}
