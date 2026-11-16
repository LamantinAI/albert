use crate::confirmations::ConfirmationWait;

pub(super) enum AgentOutcome {
    Answer(String),
    Failed(String),
    AwaitingConfirmation(ConfirmationWait),
}
impl AgentOutcome {
    pub fn into_text(self) -> String {
        match self {
            Self::Answer(text) | Self::Failed(text) => text,
            Self::AwaitingConfirmation(wait) => wait.message(),
        }
    }
}
