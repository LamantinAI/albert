//! Native per-input capabilities. Never decoded from browser/model payloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActorToolPolicy {
    NoTools,
    ExistingHost,
    ChatOnly,
}

impl ActorToolPolicy {
    pub fn intersect(self, other: Self) -> Self {
        if self == Self::NoTools || other == Self::NoTools {
            Self::NoTools
        } else if self == Self::ChatOnly || other == Self::ChatOnly {
            Self::ChatOnly
        } else {
            Self::ExistingHost
        }
    }
}
