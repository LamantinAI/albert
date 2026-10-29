//! Native per-input capabilities. Never decoded from browser/model payloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ActorToolPolicy {
    ExistingHost,
    ChatOnly,
}

impl ActorToolPolicy {
    pub fn intersect(self, other: Self) -> Self {
        if self == Self::ChatOnly || other == Self::ChatOnly {
            Self::ChatOnly
        } else {
            Self::ExistingHost
        }
    }
}
