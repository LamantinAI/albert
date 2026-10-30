//! Visible progress lifetime and ordinary-output selection. Work/history stay
//! live, and captured routes are never retargeted.
use std::{collections::HashMap, sync::Mutex};

use octo_history::SessionId;

use super::SessionContext;
use crate::db::entity::PrivateRouteKey;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ViewRoute {
    connector: String,
    channel: String,
    user: String,
}
impl ViewRoute {
    fn context(context: &SessionContext) -> Option<Self> {
        Some(Self {
            connector: context.reply_route().connector().into(),
            channel: context.reply_route().channel().into(),
            user: context.private_user_id().ok()?.as_str().into(),
        })
    }
}
/// Host-native event; it cannot be deserialized from a model payload.
#[derive(Clone, Debug)]
pub(crate) struct PresentationDetached {
    route: ViewRoute,
    session: SessionId,
    cutoff: i64,
}
impl PresentationDetached {
    pub fn session_id(&self) -> &SessionId {
        &self.session
    }
    pub fn matches(&self, context: &SessionContext) -> bool {
        ViewRoute::context(context).as_ref() == Some(&self.route)
            && context.session_id() == &self.session
            && context
                .selector_revision()
                .is_some_and(|revision| revision <= self.cutoff)
    }
}
/// Trusted committed /open event, independent of any execution envelope.
#[derive(Clone, Debug)]
pub(crate) struct PresentationOpened(pub SessionContext);
/// Host marker for navigation notices/previews, not for ordinary final replies.
#[derive(Clone, Debug)]
pub(crate) struct NavigationNotice;
/// Host output classification; model JSON cannot mint ordinary/report intent.
#[derive(Clone, Debug)]
pub(crate) struct OrdinarySessionOutput;
/// Host-only incoming intent for scheduled notifications using the turn loop.
#[derive(Clone, Debug)]
pub(crate) struct ScheduledNotification;

#[derive(Default)]
struct ViewState {
    cutoff: i64,
    current: Option<(SessionId, i64)>,
}

/// One bounded current-selection/cutoff state per verified route identity.
#[derive(Default)]
pub(crate) struct PresentationRegistry {
    cutoffs: Mutex<HashMap<ViewRoute, ViewState>>,
}
impl PresentationRegistry {
    #[cfg(test)]
    pub(crate) fn route_count(&self) -> usize {
        self.cutoffs.lock().unwrap().len()
    }
    /// Called synchronously after a committed actual selector change, before
    /// releasing navigation's lifecycle writer or publishing any detach event.
    pub fn record(
        &self,
        key: &PrivateRouteKey,
        old: &SessionId,
        old_revision: i64,
        next: &SessionId,
        next_revision: i64,
    ) {
        if old_revision <= 0 || next_revision <= old_revision {
            return;
        }
        let route = ViewRoute {
            connector: key.connector_id.clone(),
            channel: key.channel_id.clone(),
            user: key.user_id.clone(),
        };
        let mut states = self
            .cutoffs
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let state = states.entry(route).or_default();
        if old != next {
            state.cutoff = state.cutoff.max(old_revision);
        }
        Self::update_current(state, next, next_revision);
    }
    fn update_current(state: &mut ViewState, session: &SessionId, revision: i64) {
        if revision > 0
            && state
                .current
                .as_ref()
                .is_none_or(|(_, known)| revision > *known)
        {
            state.current = Some((session.clone(), revision));
        }
    }
    /// Seed from a trusted CURRENT database route row, never from a captured
    /// session context or a replay's bound journal identity.
    pub fn observe(&self, key: &PrivateRouteKey, session: &SessionId, revision: i64) {
        let route = ViewRoute {
            connector: key.connector_id.clone(),
            channel: key.channel_id.clone(),
            user: key.user_id.clone(),
        };
        let mut states = self
            .cutoffs
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        Self::update_current(states.entry(route).or_default(), session, revision);
    }
    pub fn active(&self, context: &SessionContext) -> bool {
        // Authenticated browser inputs address a Session resource directly;
        // they do not participate in a Telegram/console selector cache.
        if !context.is_private()
            || (context.reply_route().connector() == "web" && context.actor_tools.is_some())
        {
            return true;
        }
        let Some(route) = ViewRoute::context(context) else {
            return false;
        };
        self.cutoffs
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .get(&route)
            .and_then(|state| state.current.as_ref())
            .is_some_and(|(session, _)| session == context.session_id())
    }
    pub fn current(&self, context: &SessionContext) -> bool {
        let Some(route) = ViewRoute::context(context) else {
            return false;
        };
        context.is_live()
            && self
                .cutoffs
                .lock()
                .unwrap()
                .get(&route)
                .and_then(|state| state.current.as_ref())
                .is_some_and(|(session, revision)| {
                    session == context.session_id()
                        && Some(*revision) == context.selector_revision()
                })
    }
    pub fn event_for(
        &self,
        old: &SessionContext,
        next: &SessionContext,
    ) -> Option<PresentationDetached> {
        let route = ViewRoute::context(old)?;
        if ViewRoute::context(next).as_ref() != Some(&route)
            || old.session_id() == next.session_id()
        {
            return None;
        }
        let cutoff = old.selector_revision()?;
        if next.selector_revision()? <= cutoff {
            return None;
        }
        let cutoffs = self
            .cutoffs
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if cutoffs.get(&route).map(|state| state.cutoff).unwrap_or(0) < cutoff {
            return None;
        }
        Some(PresentationDetached {
            route,
            session: old.session_id().clone(),
            cutoff,
        })
    }
    pub fn detached(&self, context: &SessionContext) -> bool {
        let (Some(route), Some(revision)) =
            (ViewRoute::context(context), context.selector_revision())
        else {
            return false;
        };
        self.cutoffs
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .get(&route)
            .is_some_and(|state| revision <= state.cutoff)
    }
}
