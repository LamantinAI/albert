use albert_core::status::domain::ProgressActivity as DomainProgressActivity;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProgressActivity {
    Scratchpad,
    Memory,
    Skill,
    Connector,
    Search,
    Browser,
    Files,
    Shell,
    Sql,
    Calendar,
    Mail,
    Image,
    AudioInput,
    AudioOutput,
    Subagent,
    Sessions,
    Configuration,
    Model,
    #[serde(other)]
    Unknown,
}

impl From<DomainProgressActivity> for ProgressActivity {
    fn from(value: DomainProgressActivity) -> Self {
        match value {
            DomainProgressActivity::Scratchpad => Self::Scratchpad,
            DomainProgressActivity::Memory => Self::Memory,
            DomainProgressActivity::Skill => Self::Skill,
            DomainProgressActivity::Connector => Self::Connector,
            DomainProgressActivity::Search => Self::Search,
            DomainProgressActivity::Browser => Self::Browser,
            DomainProgressActivity::Files => Self::Files,
            DomainProgressActivity::Shell => Self::Shell,
            DomainProgressActivity::Sql => Self::Sql,
            DomainProgressActivity::Calendar => Self::Calendar,
            DomainProgressActivity::Mail => Self::Mail,
            DomainProgressActivity::Image => Self::Image,
            DomainProgressActivity::AudioInput => Self::AudioInput,
            DomainProgressActivity::AudioOutput => Self::AudioOutput,
            DomainProgressActivity::Subagent => Self::Subagent,
            DomainProgressActivity::Sessions => Self::Sessions,
            DomainProgressActivity::Configuration => Self::Configuration,
            DomainProgressActivity::Model => Self::Model,
            DomainProgressActivity::Unknown => Self::Unknown,
        }
    }
}
